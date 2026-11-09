use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use crate::dns;
use crate::dns_client;
use crate::egress;
use crate::gate;
use crate::loopback;
use crate::ls_log;
use crate::net;
use crate::proxy;
use crate::resolvers::{self, Verdict};
use crate::routes;
use crate::upstream;

// A loopback DNS relay, so the answers stay fresh.
//
// Pinning the substituted address into `hosts` works but breaks its own
// contract: xbox-dns answers with **TTL 60**, i.e. it explicitly reserves the
// right to move the address every minute, and a static file holds it forever.
//
// Instead the NRPT rules point at this listener, which relays the query - raw
// bytes, never parsed - to xbox-dns over the ISP link via IP_UNICAST_IF, and
// relays the answer back verbatim. Windows then caches it for exactly the TTL
// the resolver asked for and comes back when it expires, so a rotated address is
// picked up within a minute with no file to go stale. It behaves identically
// with and without a VPN: without one the ISP link is the default path anyway.
//
// Verified before building: the Windows DNS client does send NRPT queries to a
// 127.0.0.0/8 nameserver (a probe rule delivered the question to a listener on
// 127.0.0.53:53).
//
// UDP only, deliberately. Relaying verbatim means a truncated answer would need
// a TCP listener as well, but the routed names answer with a single A record in
// well under 100 bytes, so TC is never set. If that ever changes, Windows falls
// back to the next nameserver in the rule rather than failing outright.

/// Where the NRPT rules point. Any 127.0.0.0/8 address works; .53 keeps it
/// recognisable and clear of anything bound to 127.0.0.1.
pub const LISTEN_IP: &str = "127.0.0.53";
pub const LISTEN_PORT: u16 = 53;

/// What generation of relay this build ships.
///
/// Separate from the product version on purpose: the relay is installed once,
/// by an administrator, and then keeps running across reboots from
/// `%ProgramData%` - so a user can be running a months-old relay under a fresh
/// app and never be told. The app compares this against what the
/// installed relay wrote and says so.
///
/// **Bump this whenever the relay's own behaviour changes** - the loop, the
/// resolver logic it carries, the warm loop, the watchdog. A pure UI or patcher
/// change does not need it. Never decrease it: the comparison is "older than",
/// and a version that goes backwards asks every user to reinstall.
///
/// 1 = first versioned relay: answer cache + warm loop.
/// 2 = a non-substituted answer can no longer pin the client for its own TTL.
/// 3 = carries the fallback proxy (`proxy.rs`) alongside the DNS relay.
/// 4 = the proxy no longer answers a socket the client has not spoken on.
/// 5 = identity and telemetry hosts are tunnelled, never intercepted.
/// 6 = liveness is re-probed on the warm loop, so a dead address is dropped
///     within seconds instead of being advertised for ten minutes.
/// 7 = a provider choice is only remembered when it actually substituted.
/// 8 = the race log is client-only, so it stops eating its own log budget.
/// 9 = the warm loop gets a generous liveness budget; the client path stays tight.
/// 10 = `enable()` verifies the relay came up instead of trusting the task.
/// 11 = liveness is measured by the TLS handshake, not just the connection.
/// 12 = the fallback route picks its upstream by measured handshake latency.
/// 13 = the byte pump is non-blocking, so it stops adding its own latency.
/// 14-16 = no separate entries were kept for these.
/// 17 = the proxy carries the gate hosts through the cert-free relay route.
/// 18 = the TLS-terminating carrier route is gone with its CA, and the warm loop
///     no longer measures upstreams (there is one relay, nothing to choose).
/// 19 = idle relayed tunnels are closed before they go stale, the relay is
///     benched when it stops carrying, teardown is logged and lines are stamped.
/// 20 = a relay that cuts tunnels at the handshake counts as failing (bytes
///     moved, so "carried nothing" missed it), and the bench backs off.
/// 21 = the relay leg asks for `cloudcode-pa` instead of `daily-`, which costs
///     it a second proxy hop: 2.14 s median down to 0.22 s, and no variance.
/// 22 = the route is checked by a probe on the warm loop instead of by someone's
///     request, and a burst of in-flight failures no longer lengthens the bench.
/// 23 = every child process it shells out to is bounded, so a hung helper can no
///     longer stop it dead.
/// 27 = routes are ordered by measured speed (`routes`), the client's own log is
///     tailed for the region 400 (`ls_log`) and answered by forcing substitution
///     back on and penalising the route it came through, and the direct route
///     fails inside a budget instead of hanging on the OS connect timeout.
/// 28 = it writes down what it is doing (`gate.json`): the leader route, the
///     forced-substitution deadline, the stand-down verdict, and what it did
///     about the last region 400. Without this generation the window can see
///     the refusal in the client's log but nothing about the answer, so the
///     bump is what tells the user their service is too old to report (S48).
/// 29 = the DoH provider keeps its connections (`doh::Pool`) instead of opening
///     one per query, and each address resumes its own TLS session. dns-ai.ru
///     measured 1.06 queries per connection and its CPU going to handshakes;
///     the relay is where those queries come from, so it is what has to change.
///     Same generation: a third dns-ai.ru node (msk1) in the walk.
/// 30 = carries the gate hosts itself on every network (2.14.0_1): answers them
///     with its own loopback addresses (`loopback`), so a client started before
///     the proxy variable is routed too (G50); pins its own connections to the
///     ISP link while a VPN holds the default route, DoH included (N25, P13);
///     ranks routes by the model answers they carried, not only by speed, and
///     closes a refused route's tunnels (`routes`); watches the client logs
///     every two seconds, the CLI's included; answers AAAA/HTTPS for the gate
///     hosts with nothing (P40); and no longer stands down for a VPN while that
///     door is up (D25) - the VPN is a route of its own. xbox-dns.ru is out of
///     the pool.
/// 31 = its watchdog patches a fresh install too, not only one an update
///     reverted: gated on auto-patch minus a patch declined by hand instead of
///     on the user having patched once (D27), re-scanning for installs every
///     10 s instead of 5 min, the user's own paths included. An older relay
///     leaves a newly installed Antigravity unpatched with auto-patch on.
/// 32 = says what keeps its listeners down (a program holding the port, a
///     port Windows reserved, security software refusing it - `portcheck`),
///     retries them every minute, moves the local proxy off a port it cannot
///     have (P26), and reports when nothing on the internet answers it
///     (`gate::Report::{blockers, reached_at, started_at, exe}`, P53). An older
///     relay leaves a user with `os error 10013` in its log and a 400 on screen.
/// 33 = its own UDP `:53` is retried and diagnosed like the other two listeners
///     instead of killing the process (`serve_dns_forever`), so a machine where
///     something holds port 53 still gets the door, the proxy, the exits and
///     the auto-patch; a refusal no longer benches - or even disconnects - a
///     route that carried a model answer in the last ninety seconds
///     (`routes::Blamed::Proven`), the first bench is two minutes rather than
///     ten, a region bench orders a route instead of removing it from the table
///     for every kind alike, and a route that fails its probe is kept back
///     until one succeeds. An older relay drops the working route mid-answer
///     and thrashes between benched ones.
/// 37 = a refusal the table declines to hold against a route no longer costs it
///     its place in the order either (`routes::bad_at` split from
///     `refused_at`, G76), the connection a refusal arrived on is named in the
///     log and closed when it is not the one carrying the answer, and the log
///     says which host was refused and how many answers were held back. An
///     older relay swaps the gate route away from the one that is answering
///     every fifteen seconds and cannot say whether a route half-works.
/// 39 = a live gate tunnel the client spoke into and got nothing back from
///     benches its route (`routes::SILENT_STEPS`), and a passing probe no
///     longer lets it back in front - only a model answer does. An older relay
///     keeps sending every connection to a built-in exit that opens and then
///     carries nothing, and never tries the DNS route behind it.
/// 40 = the user's own proxy gives way while benched (region 400, silent
///     tunnel, failed open) instead of staying first regardless. An older
///     relay answers a region 400 through the user's proxy by cutting its
///     tunnels and sending the retry straight back into it. Same generation:
///     the user's proxy no longer has its exit region traced on the warm loop
///     (nor when it is added) - the region-400 watch stands it down instead.
pub const RELAY_VERSION: u32 = 40;

/// Written where an unelevated relay can write and an unelevated app can
/// read. Absent means a relay from before versioning, i.e. older than anything.
const VERSION_FILE: &str = "relay.version";

/// Closes the console Windows hands a console subsystem process. Without this
/// the scheduled task leaves an empty black window on screen for as long as the
/// relay lives, which is forever. Everything it has to say goes to the log file
/// anyway, so losing stdout costs nothing.
#[cfg(target_os = "windows")]
pub fn detach_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn FreeConsole() -> i32;
    }
    unsafe {
        FreeConsole();
    }
}

#[cfg(not(target_os = "windows"))]
pub fn detach_console() {}

/// Log prefix for what the answer turned out to be. `substituted` is the only
/// one that means the region gate is actually being defeated for that name;
/// `PASSTHROUGH` shouts because it is the failure the old `ok` used to hide.
fn verdict_tag(v: Verdict) -> &'static str {
    match v {
        Verdict::Substituted => "substituted",
        Verdict::Sibling => "sibling",
        Verdict::Passthrough => "PASSTHROUGH",
        Verdict::Unknown => "ok",
    }
}

/// How long a *detected* interface is trusted. Long on purpose: detection
/// shells out to PowerShell, and an interface that dies is caught by
/// `invalidate_interface()` on the first failed relay, so re-probing on a timer
/// buys nothing and only spawns processes on the user's machine.
const EGRESS_TTL: Duration = Duration::from_secs(30 * 60);
/// How long a *failed* detection is remembered. Short, because it is usually
/// the network not being up yet at logon - but not zero, or a machine with no
/// physical egress at all would spawn a probe per query.
const EGRESS_RETRY: Duration = Duration::from_secs(30);
/// How often the relay re-asks whether a tunnel is carrying the machine.
///
/// Its own clock, and deliberately far shorter than `EGRESS_TTL`: an interface
/// index changes when hardware does, but a VPN is something the user toggles
/// mid-session, and that answer decides whether the relay substitutes at all
/// (G26). Costs one PowerShell spawn per interval and never runs on the query
/// path - a client waits on nothing here. Four minutes rather than fifteen
/// seconds because being late merely means a few minutes of the old, slightly
/// longer route; it breaks nothing.
const VPN_CHECK_EVERY: Duration = Duration::from_secs(4 * 60);
const LOG_LIMIT_BYTES: u64 = 64 * 1024;

/// Interface index, when it was learned, and how long that answer is good for.
static EGRESS_CACHE: Mutex<Option<(u32, Instant, Duration)>> = Mutex::new(None);

/// Р–СѓСЂРЅР°Р» Рё РІСЃС‘ СЃРѕСЃС‚РѕСЏРЅРёРµ. Р’ РїРѕСЂС‚Р°С‚РёРІРЅРѕРј СЂРµР¶РёРјРµ вЂ” `data\` СЂСЏРґРѕРј СЃ exe (С„Р»РµС€РєР°);
/// РІ РѕР±С‹С‡РЅРѕРј вЂ” РїРѕРґ РїСЂРѕС„РёР»РµРј РїРѕР»СЊР·РѕРІР°С‚РµР»СЏ, Р° РЅРµ СЂСЏРґРѕРј СЃ exe: СЂРµР»РµР№ СЂР°Р±РѕС‚Р°РµС‚
/// Р±РµР· РїРѕРІС‹С€РµРЅРёСЏ РїСЂР°РІ, Рё РєР°С‚Р°Р»РѕРі, РєСѓРґР° Р°РґРјРёРЅРёСЃС‚СЂР°С‚РѕСЂ СЃС‚Р°РІРёР» РїСЂРѕРіСЂР°РјРјСѓ, РµРјСѓ
/// РЅРµРґРѕСЃС‚СѓРїРµРЅ РЅР° Р·Р°РїРёСЃСЊ.
#[cfg(target_os = "windows")]
pub fn log_dir() -> PathBuf {
    if let Some(dir) = crate::portable::data_dir() {
        return dir;
    }
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("OpenAntigravity")
}

/// РџРѕСЂС‚Р°С‚РёРІРЅС‹Р№ СЂРµР¶РёРј РїСЂРёРѕСЂРёС‚РµС‚РµРЅ Рё РЅР° Linux: РІСЃС‘ СЃРѕСЃС‚РѕСЏРЅРёРµ вЂ” СЂСЏРґРѕРј СЃ exe,
/// РѕР±С‹С‡РЅС‹Р№ РїСѓС‚СЊ вЂ” XDG (`~/.local/share/openantigravity`).
#[cfg(not(target_os = "windows"))]
pub fn log_dir() -> PathBuf {
    if let Some(dir) = crate::portable::data_dir() {
        return dir;
    }
    let base = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{}/.local/share", home)
        });
    PathBuf::from(base).join("openantigravity")
}

pub fn log_path() -> PathBuf {
    log_dir().join("forwarder.log")
}

pub fn version_path() -> PathBuf {
    log_dir().join(VERSION_FILE)
}

/// Records which relay generation is in place. Called both by the installer and
/// by the relay itself at startup: the installer's write is what makes the
/// answer correct the moment an upgrade finishes, and the relay's write is what
/// keeps it honest if the exe ever gets there some other way.
pub fn record_version() {
    if let Some(dir) = version_path().parent() {
        fs::create_dir_all(dir).ok();
    }
    fs::write(version_path(), RELAY_VERSION.to_string()).ok();
}

/// The relay generation currently installed. `0` for a relay old enough not to
/// have written one - which is exactly the case worth reporting.
pub fn installed_version() -> u32 {
    parse_version(fs::read_to_string(version_path()).ok().as_deref())
}

/// Anything unreadable counts as the oldest possible relay: the file is written
/// by us and never edited, so a value that will not parse means something else
/// wrote it, and "reinstall" is the right answer to that too.
fn parse_version(raw: Option<&str>) -> u32 {
    raw.and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

/// Local wall-clock `HH:MM:SS` for a log line.
///
/// Local, not UTC: the only reader is a user comparing the log against the moment
/// their editor showed an error, and asking them to do timezone arithmetic on a
/// bug report is how a report becomes useless. Empty where there is no local
/// clock to read (`utils::local_clock`), which costs a stamp rather than a line.
fn stamp() -> String {
    crate::utils::local_clock().map_or_else(String::new, |c| c.hms())
}

/// Best-effort logging: a background process with no console is otherwise
/// impossible to diagnose. Truncated rather than rotated - nothing here is
/// worth keeping across sessions.
///
/// Every line is stamped. Without it a log says what happened but never *when*,
/// so a torn connection cannot be lined up against the error the user saw - which
/// is precisely the question a bug report asks.
fn log(line: &str) {
    // Never from a test. The file is the *installed* relay's log, and a unit test
    // exercising `health` or `routes` used to append lines like В«С‚РµСЃС‚ РѕС‚Р»РѕР¶РµРЅ РЅР°
    // 5 РјРёРЅВ» to it on the developer's machine - which the window's В«РЎРѕС…СЂР°РЅРёС‚СЊ
    // РѕС‚С‡С‘С‚В» now puts in a support file (G18's other half).
    if cfg!(test) {
        return;
    }
    let (note, write_line) = collapse_repeat(line);
    if note.is_none() && !write_line {
        return;
    }
    let path = log_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).ok();
    }
    if fs::metadata(&path).is_ok_and(|m| m.len() > LOG_LIMIT_BYTES) {
        fs::remove_file(&path).ok();
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        // One write per line. `writeln!` with arguments issues a write per
        // piece, and an append is atomic only per write, so two threads (or two
        // processes) logging at once interleaved into lines like
        // В«16:48:2616:48:26 PASSTHROUGH вЂ¦ PASSTHROUGH вЂ¦В» - seen in reports from
        // both platforms.
        let at = stamp();
        let mut entry = String::new();
        if let Some(note) = note {
            entry.push_str(&format!("{} {}\n", at, note));
        }
        if write_line {
            entry.push_str(&format!("{} {}\n", at, line));
        }
        f.write_all(entry.as_bytes()).ok();
    }
}

/// How long a line has to repeat within to be counted rather than written.
const REPEAT_WINDOW: Duration = Duration::from_secs(30);
/// вЂ¦and how many repeats are counted before the tally is written anyway, so a
/// problem repeating forever is never silent.
const REPEAT_RUN: u32 = 50;

/// Collapses a run of one identical line into a count.
///
/// The log holds 64 KB and the report shows its last 60 lines, so a burst of
/// one message costs exactly the minutes a report is about: a field report
/// (2026-09-21) spent fifteen of its sixty visible lines on the same
/// В«РЅР°РїСЂСЏРјСѓСЋ вЂ¦: РёРјСЏ РЅРµ СЂР°Р·СЂРµС€Р°РµС‚СЃСЏВ» inside a single second, and the route
/// switch that followed was off the top of it.
///
/// Returns the tally line to write first, if any, and whether `line` itself is
/// written. The *first* of a run always goes out at once - the news is that it
/// happened, and a delayed first line is a log that lies about when.
fn collapse_repeat(line: &str) -> (Option<String>, bool) {
    static RUN: Mutex<Option<(String, u32, Instant)>> = Mutex::new(None);
    let tally = |n: u32| format!("в†‘ С‚Рѕ Р¶Рµ СЃР°РјРѕРµ РµС‰С‘ {} СЂР°Р·", n);
    let Ok(mut g) = RUN.lock() else {
        return (None, true);
    };
    let same = g
        .as_ref()
        .is_some_and(|(prev, _, at)| prev == line && at.elapsed() < REPEAT_WINDOW);
    if same {
        let mut flush = None;
        if let Some((_, n, at)) = g.as_mut() {
            *n += 1;
            *at = Instant::now();
            if *n >= REPEAT_RUN {
                flush = Some(tally(*n));
                *n = 0;
            }
        }
        return (flush, false);
    }
    let held = g.as_ref().map_or(0, |(_, n, _)| *n);
    *g = Some((line.to_string(), 0, Instant::now()));
    ((held > 0).then(|| tally(held)), true)
}

/// The only way a failed start can be reported: the console is gone by then.
pub fn log_fatal(message: &str) {
    log(&format!("fatal: {}", message));
}

/// The proxy shares the relay's log - it runs in the same process, and one file
/// is what makes a resolve and the connection that followed it readable together.
pub fn log_proxy(message: &str) {
    log(&format!("proxy        {}", message));
}

/// Who answered a race that produced no substitution, and whether a reference
/// was available to judge it. Only logged on that path: it is the one where the
/// interesting question is which provider was missing, and it is rare enough
/// that a line per occurrence is affordable.
pub fn log_race(name: &str, heard: &[&str], had_reference: bool) {
    log(&format!(
        "race         {} heard [{}]{}",
        name,
        heard.join(", "),
        if had_reference { "" } else { " (no reference)" }
    ));
}

fn invalidate_interface() {
    if let Ok(mut c) = EGRESS_CACHE.lock() {
        *c = None;
    }
}

fn isp_interface() -> u32 {
    if let Ok(cache) = EGRESS_CACHE.lock() {
        if let Some((idx, at, good_for)) = *cache {
            if at.elapsed() < good_for {
                return idx;
            }
        }
    }
    // 0 means "use the routing table" - the right degradation when there is no
    // physical egress to name, but a poor thing to remember for long: at logon
    // the network is often simply not up yet, and caching the miss would leave
    // half an hour of unsubstituted answers.
    let (idx, good_for) = match egress::detect() {
        Some(eg) => (eg.if_index, EGRESS_TTL),
        None => (0, EGRESS_RETRY),
    };
    if let Ok(mut cache) = EGRESS_CACHE.lock() {
        *cache = Some((idx, Instant::now(), good_for));
    }
    idx
}

/// Relays one query and reports which provider answered and whether that answer
/// was actually substituted.
///
/// The choice is no longer a constant. A provider can drop a name from its
/// list without any error - the query still resolves, just to the genuine
/// Google address - so every provider is asked at once and compared against a
/// reference resolver that substitutes nothing. See `resolvers`.
fn relay(query: &[u8]) -> Option<(Vec<u8>, &'static str, resolvers::Verdict)> {
    let if_index = isp_interface();
    match resolvers::resolve_best(query, if_index) {
        Some(hit) => Some(hit),
        None => {
            // Nobody answered: the interface may have gone away under us, so
            // the next query re-detects instead of retrying a dead one.
            invalidate_interface();
            None
        }
    }
}

/// How often the routed names are re-resolved in the background.
///
/// Half the substituted TTL, so an entry is always well inside the window the
/// answer cache will serve it from and a client query never finds it expired.
/// Cheap: four names, one upstream query each, once every quarter minute.
const WARM_EVERY: Duration = Duration::from_secs(15);

/// Keeps a vetted answer ready for every routed name, forever.
///
/// The relay has about a second to answer before Windows asks the next
/// nameserver in the NRPT rule - which is a provider's own resolver, handing
/// out addresses that nothing has checked for liveness. A cold resolution does
/// not reliably fit in that second (race, then a liveness probe, and a provider
/// that goes quiet costs the whole timeout), and it does not have to: doing the
/// work on a timer instead means the client's query is answered from memory.
/// How often the user's own proxy, the built-in exits and the direct route are
/// re-timed while healthy. Rare, because a working route needs no supervision
/// and each probe is a real request. The relay is deliberately kept off this
/// clock - see the warm loop below for why.
const PROBE_HEALTHY_EVERY: Duration = Duration::from_secs(2 * 60);

/// Where the relay asks the routing table to send a packet, to notice a tunnel
/// coming up or going down between the full scans. Any public address works;
/// this one is only ever used as a route lookup, never contacted.
const ROUTE_PROBE_DEST: std::net::Ipv4Addr = std::net::Ipv4Addr::new(8, 8, 8, 8);

/// How often the tunnel's exit country is re-read while a VPN stays up. A
/// VPN client can switch servers without the route table noticing anything.
const VPN_EXIT_EVERY: Duration = Duration::from_secs(30 * 60);

/// The tunnel's exit country, as last measured; empty when unknown.
static VPN_EXIT_COUNTRY: Mutex<String> = Mutex::new(String::new());
static MEASURING_EXIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Reads where the tunnel comes out - Cloudflare's trace over the default
/// route, which is the tunnel - and decides whether the `Vpn` route may be
/// offered. On a thread: it is a real HTTPS request, and the warm pass has
/// budgets of its own.
fn measure_vpn_exit() {
    use std::sync::atomic::Ordering;
    if MEASURING_EXIT.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = thread::Builder::new()
        .name("vpn-exit".to_string())
        .spawn(|| {
            let exit = upstream::machine_exit();
            let (country, permitted) = match &exit {
                Some((_, loc)) => (loc.clone(), Some(!upstream::region_is_blocked(loc))),
                None => (String::new(), None),
            };
            // Only while a tunnel is still up: a measurement that raced the VPN
            // going down would otherwise offer a route that no longer exists.
            if net::tunnel_up() {
                proxy::set_vpn_exit(permitted);
                if let Ok(mut c) = VPN_EXIT_COUNTRY.lock() {
                    *c = country.clone();
                }
                log(&match permitted {
                    Some(true) => format!(
                        "VPN РІС‹С…РѕРґРёС‚ РІ {} вЂ” РµРіРѕ РјРѕР¶РЅРѕ РёСЃРїРѕР»СЊР·РѕРІР°С‚СЊ РєР°Рє РјР°СЂС€СЂСѓС‚",
                        country
                    ),
                    Some(false) => format!(
                        "VPN РІС‹С…РѕРґРёС‚ РІ {} вЂ” С‚Р°Рј РѕС€РёР±РєР° 400, СЌС‚РѕС‚ РјР°СЂС€СЂСѓС‚ РЅРµ РёСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ",
                        country
                    ),
                    None => "СЃС‚СЂР°РЅСѓ РІС‹С…РѕРґР° VPN СѓР·РЅР°С‚СЊ РЅРµ СѓРґР°Р»РѕСЃСЊ".to_string(),
                });
            }
            MEASURING_EXIT.store(false, Ordering::SeqCst);
        });
    if spawned.is_err() {
        MEASURING_EXIT.store(false, Ordering::SeqCst);
    }
}

fn vpn_exit_country() -> String {
    VPN_EXIT_COUNTRY
        .lock()
        .map(|c| c.clone())
        .unwrap_or_default()
}

/// A fingerprint of the network the route table's evidence belongs to: which
/// adapter is the ISP link, which one the default route leaves through, and
/// whether that is a tunnel. Never 0 - that is the table's "not set yet".
fn network_fingerprint(isp: u32, best: Option<u32>, tunnel: bool) -> u64 {
    1 + ((isp as u64) << 33) + ((best.unwrap_or(0) as u64) << 1) + tunnel as u64
}

fn warm_forever() {
    let mut since_upstream = PROBE_HEALTHY_EVERY;
    let mut since_exits = PROBE_HEALTHY_EVERY;
    let mut since_direct = PROBE_HEALTHY_EVERY;
    let mut since_vpn = VPN_CHECK_EVERY;
    let mut since_exit_check = Duration::ZERO;
    let mut last_best: Option<u32> = None;
    loop {
        // One small record per pass, for a window that is another process and
        // can otherwise only read the log meant for a person (P29). **First** in
        // the pass, not last: a pass opens with a PowerShell VPN check and runs
        // several probes with budgets of their own, so written at the end the
        // file would not exist for the first tens of seconds of the relay's life
        // - which is exactly when a user has just switched the bypass on and is
        // watching the window. The leader is the one the last pass settled on,
        // which is the one in force right now.
        let exit_country = vpn_exit_country();
        gate::publish(gate::Now {
            route: routes::leader().map(|k| k.label()),
            forced_for: resolvers::substitution_forced_for(),
            stood_down: resolvers::tunnel_carries_client(),
            loopback: loopback::active(),
            tunnel: net::tunnel_up(),
            vpn_exit: &exit_country,
            routes: routes::rows(|k| proxy::route_usable(k, ROUTE_PROBE_HOST)),
        });
        // One syscall, every pass: where the default route goes now. A change
        // is a VPN coming up or going down, or a different network - worth the
        // full scan right away rather than on the four-minute clock.
        let best = net::best_interface(ROUTE_PROBE_DEST);
        if best != last_best {
            if last_best.is_some() {
                invalidate_interface();
            }
            last_best = best;
            since_vpn = VPN_CHECK_EVERY;
        }
        // First, because everything below depends on it: when a tunnel carries
        // the client the relay stops substituting and answers as the tunnel's
        // own resolver would. The rules cannot be removed from here - the task
        // runs at `RunLevel Limited` and NRPT needs an administrator - so the
        // relay changes what it *answers* instead, which needs no privilege and
        // reaches the same place. Menu 1 removes the rules outright on the next
        // elevated run (`dns::refresh_pinned_hosts`).
        if since_vpn >= VPN_CHECK_EVERY {
            // A tunnel holding the default route no longer takes the bypass down
            // (D25): the client's gate connections come to us on loopback, so the
            // tunnel can only ever catch *our* connections - and those are pinned
            // to the ISP link while it is up, the same way every resolver query
            // already was (I4). The user's VPN is still used, as a route of its
            // own (`routes::Kind::Vpn`), whenever it exits somewhere the gate
            // does not apply.
            let eg = egress::detect();
            let tunnel = eg.as_ref().is_some_and(|e| e.vpn_active);
            let was = net::tunnel_up();
            net::set_pin_interface(match &eg {
                Some(e) if tunnel => e.if_index,
                _ => 0,
            });
            // The stand-down survives for exactly one case: the loopback door is
            // down, so the client dials Google itself, and it is measured inside
            // the tunnel. Then the tunnel is its only way out, and a substituted
            // address reached from the tunnel's exit is refused by the RU-only
            // providers (N25) - so the relay answers as the tunnel would (D13's
            // old rule, now the fallback D25 leaves in place).
            let (stand_down, _) = if tunnel && !loopback::active() {
                egress::vpn_verdict(eg.as_ref())
            } else {
                (false, egress::ClientEgress::Unknown)
            };
            if stand_down != resolvers::tunnel_carries_client() {
                log(if stand_down {
                    "Antigravity С…РѕРґРёС‚ С‡РµСЂРµР· VPN РјРёРјРѕ РѕР±С…РѕРґР° вЂ” Р°РґСЂРµСЃР° РѕС‚РґР°СЋС‚СЃСЏ РєР°Рє Сѓ VPN"
                } else {
                    "РїРѕРґРјРµРЅР° Р°РґСЂРµСЃРѕРІ СЃРЅРѕРІР° РІРєР»СЋС‡РµРЅР°"
                });
            }
            resolvers::set_vpn_active(stand_down);
            if tunnel != was {
                log(if tunnel {
                    "VPN РїРѕРґРЅСЏС‚ вЂ” СЃРѕРµРґРёРЅРµРЅРёСЏ СЃР»СѓР¶Р±С‹ СЃ СЃРµСЂРІРёСЃР°РјРё СЂР°Р·Р±Р»РѕРєРёСЂРѕРІРєРё РёРґСѓС‚ РјРёРјРѕ РЅРµРіРѕ, С‡РµСЂРµР· РїСЂРѕРІР°Р№РґРµСЂР°"
                } else {
                    "VPN РѕС‚РєР»СЋС‡С‘РЅ"
                });
            }
            if tunnel && (!was || proxy::vpn_exit().is_none()) {
                measure_vpn_exit();
                since_exit_check = Duration::ZERO;
            }
            if !tunnel {
                proxy::set_vpn_exit(None);
                if let Ok(mut c) = VPN_EXIT_COUNTRY.lock() {
                    c.clear();
                }
            }
            since_vpn = Duration::ZERO;
        }
        if net::tunnel_up() && since_exit_check >= VPN_EXIT_EVERY {
            measure_vpn_exit();
            since_exit_check = Duration::ZERO;
        }
        let egress = isp_interface();
        // Not while the ISP interface is unknown: `isp_interface` answers 0 for
        // half a minute after a failed detection, and taking that for a new
        // network wiped every bench - and put back first the route the gate had
        // just refused - twice per hiccup.
        if egress != 0 && routes::set_context(network_fingerprint(egress, best, net::tunnel_up())) {
            log_proxy("СЃРµС‚СЊ РёР·РјРµРЅРёР»Р°СЃСЊ вЂ” С‡С‚Рѕ СЂР°Р±РѕС‚Р°РµС‚, РІС‹СЏСЃРЅСЏРµС‚СЃСЏ Р·Р°РЅРѕРІРѕ");
        }
        // The client logs are watched on a thread of their own every two
        // seconds (`watch_client_logs`), not here: a refusal costs the user
        // every request until it is answered, and a warm pass is fifteen
        // seconds plus its probes.
        resolvers::warm(dns::core_namespaces(), egress);
        // The relay is somebody else's server, so it is the one route we never
        // probe on a timer. A health check every two minutes, from every machine
        // running this tool, is precisely the `handshake_completed` beacon a relay
        // operator can count and attribute (kb/rivals.md) - and the "considerate
        // guest" rule the built-in exits follow just below applies with far more
        // force to a route we do not own. Left unmeasured, the route table already
        // treats it as a last resort (an unmeasured route sorts after every
        // measured one); it is touched unbidden only to lift a bench a real
        // failure set, so a transient fault is not made permanent.
        if proxy::relay_is_benched() {
            proxy::probe_relay();
        }
        // The user's own proxy is checked the same way and for the same reason:
        // it must be stood down before a request meets it, and picked back up
        // the moment it works again - which is what they asked for when they
        // gave us one.
        if upstream::OWN.health.is_benched() || since_upstream >= PROBE_HEALTHY_EVERY {
            upstream::probe_health();
            since_upstream = Duration::ZERO;
        }
        // The built-in exits are checked on the same clock in *both* states, and
        // that is the one place this deviates from the rule above. Probing a
        // benched route every pass is right when the alternative is the seven-second
        // DNS route; these sit above the relay, so what a slower revival costs is a
        // route that is merely quicker - and the cost of the other choice is a
        // connection every fifteen seconds, from every machine running this tool,
        // to somebody's free proxy. Being a considerate guest is what keeps the
        // route working at all.
        if since_exits >= PROBE_HEALTHY_EVERY {
            proxy::probe_exits();
            since_exits = Duration::ZERO;
        }
        // The direct route is timed on the same clock as the others, so the
        // route table compares like with like. Not more often while penalised:
        // a region penalty is a clock, and no measurement shortens it.
        if since_direct >= PROBE_HEALTHY_EVERY {
            proxy::probe_direct(egress);
            proxy::probe_vpn();
            since_direct = Duration::ZERO;
        }
        routes::refresh_leader(|k| proxy::route_usable(k, ROUTE_PROBE_HOST));
        thread::sleep(WARM_EVERY);
        since_upstream += WARM_EVERY;
        since_exits += WARM_EVERY;
        since_direct += WARM_EVERY;
        since_vpn += WARM_EVERY;
        since_exit_check += WARM_EVERY;
    }
}

/// How often the client logs are looked at. Two seconds: a user who sees the
/// error and presses "send" again should find the route already switched.
const LOG_WATCH_EVERY: Duration = Duration::from_secs(2);

/// The one place the region 400 and the model answer are written down is the
/// client's own log (`ls_log`). Looked at every two seconds; reading a file's
/// tail when it has not grown costs one `metadata`.
fn watch_client_logs() {
    loop {
        thread::sleep(LOG_WATCH_EVERY);
        let hits = ls_log::poll();
        if !hits.is_empty() {
            answer_log(&hits);
        }
    }
}

/// The name the route table is refreshed against: the gate host the IDE uses.
const ROUTE_PROBE_HOST: &str = "daily-cloudcode-pa.googleapis.com";

/// Answers what the client logs say: a refusal benches the route that carried
/// it and closes its tunnels, an answer proves the route that carried it.
///
/// The older of the two is handled first, so a pass that saw a refusal and then
/// an answer on the route that replaced it ends with the right route proven -
/// and one that saw an answer and then a refusal ends with it benched.
///
/// Nothing is attributed on a guess: `routes::attribute` names the route whose
/// gate tunnel was open around the line's own stamp, and with none open the
/// refusal is one the client made without us (it dialled Google itself), which
/// no route switch can help with - so the DNS layer re-races instead, the only
/// lever that reaches such a client.
fn answer_log(hits: &[(std::path::PathBuf, ls_log::Tally)]) {
    let mut refusals = 0usize;
    let mut answers = 0usize;
    let mut refused_ago: Option<Duration> = None;
    let mut answered_ago: Option<Duration> = None;
    let mut refused_host: Option<String> = None;
    let mut answered_host: Option<String> = None;
    for (path, t) in hits {
        let file = path
            .file_name()
            .map_or_else(String::new, |f| f.to_string_lossy().into_owned());
        if t.refusals > 0 {
            // The host, when the log named one: a refusal about a name we do
            // not route at all is a different story from one on a gate host,
            // and the line said neither (three field reports, 2026-09-21).
            log_proxy(&format!(
                "region-400 x{} РІ {}{} вЂ” Antigravity СѓРїС‘СЂСЃСЏ РІ РіРµР№С‚",
                t.refusals,
                file,
                t.refused_host
                    .as_deref()
                    .map_or_else(String::new, |h| format!(" ({h})"))
            ));
        }
        refusals += t.refusals;
        answers += t.answers;
        if t.refused_ago.is_some() && newest(refused_ago, t.refused_ago) == t.refused_ago {
            refused_host = t.refused_host.clone();
        }
        if t.answered_ago.is_some() && newest(answered_ago, t.answered_ago) == t.answered_ago {
            answered_host = t.answered_host.clone();
        }
        refused_ago = newest(refused_ago, t.refused_ago);
        answered_ago = newest(answered_ago, t.answered_ago);
    }
    // Only an event that can be placed in time is acted on. A line whose stamp
    // will not parse, or one older than the tunnels are remembered, would be
    // pinned on whatever route is open *now* - and bench a route that never saw
    // it (review finding before 2.14.0_1).
    let when = |ago: Option<Duration>| {
        ago.filter(|a| *a <= routes::TUNNEL_MEMORY)
            .and_then(|a| Instant::now().checked_sub(a))
    };
    let refusal = (refusals > 0).then(|| when(refused_ago)).flatten();
    let answer = (answers > 0).then(|| when(answered_ago)).flatten();
    match (refusal, answer) {
        (Some(r), Some(a)) if a < r => {
            on_answers(answers, a, answered_host.as_deref());
            on_refusals(refusals, r, refused_host.as_deref());
        }
        (r, a) => {
            if let Some(r) = r {
                on_refusals(refusals, r, refused_host.as_deref());
            }
            if let Some(a) = a {
                on_answers(answers, a, answered_host.as_deref());
            }
        }
    }
}

fn newest(a: Option<Duration>, b: Option<Duration>) -> Option<Duration> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// One line about the connection a refusal came in on: the fact that decides
/// whether changing the route could possibly help.
///
/// Same connection as the one carrying an answer в†’ the backend is turning
/// single requests down and every road leads to the same place. A sibling в†’
/// this is one pooled connection that went bad while another works, and closing
/// it is what makes the client dial a fresh one (D26). The log could not tell
/// the two apart, and three field reports (2026-09-21) turn on exactly that:
/// a route answering and refusing in the same minute, over and over.
fn connection_note(kind: routes::Kind, id: u64, shape: Option<routes::Shape>) -> String {
    let Some(s) = shape else {
        return format!("region-400 РїРѕ СЃРѕРµРґРёРЅРµРЅРёСЋ #{} В«{}В»", id, kind.label());
    };
    let open = routes::open_counts();
    let others: Vec<String> = routes::ALL
        .iter()
        .zip(open)
        .filter(|(_, n)| *n > 0)
        .map(|(k, n)| format!("{} {}", k.label(), n))
        .collect();
    let what = match s.answered_ago {
        Some(ago) => format!(
            "С‚Рѕ Р¶Рµ СЃРѕРµРґРёРЅРµРЅРёРµ, РїРѕ РєРѕС‚РѕСЂРѕРјСѓ С€С‘Р» РѕС‚РІРµС‚ ({} СЃ РЅР°Р·Р°Рґ, {} РљР‘)",
            ago.as_secs(),
            s.to_client / 1024
        ),
        None => format!(
            "РѕС‚РІРµС‚РѕРІ РЅРµ РЅС‘СЃ, РѕС‚РєСЂС‹С‚Рѕ {} СЃ, {} РљР‘",
            s.age.as_secs(),
            s.to_client / 1024
        ),
    };
    format!(
        "region-400 РїРѕ СЃРѕРµРґРёРЅРµРЅРёСЋ #{} В«{}В»: {}; РѕС‚РєСЂС‹С‚Рѕ: {}",
        id,
        kind.label(),
        what,
        others.join(", ")
    )
}

fn on_refusals(count: usize, at: Instant, host: Option<&str>) {
    let carried = routes::attribute_tunnel(at, host);
    let mut acted: Vec<String> = Vec::new();
    match carried {
        Some((kind, tunnel)) => {
            let shape = routes::tunnel_shape(tunnel);
            log_proxy(&connection_note(kind, tunnel, shape));
            match routes::blame(kind) {
                routes::Blamed::Benched(penalty) => {
                    log_proxy(&format!(
                        "РјР°СЂС€СЂСѓС‚ В«{}В» РЅС‘СЃ region-400 вЂ” РѕС‚Р»РѕР¶РµРЅ РЅР° {}, РµРіРѕ СЃРѕРµРґРёРЅРµРЅРёСЏ Р·Р°РєСЂС‹С‚С‹",
                        kind.label(),
                        human_minutes(penalty)
                    ));
                    acted.push(format!(
                        "РјР°СЂС€СЂСѓС‚ В«{}В» РѕС‚Р»РѕР¶РµРЅ РЅР° {}, СЃР»РµРґСѓСЋС‰РµРµ СЃРѕРµРґРёРЅРµРЅРёРµ РїРѕР№РґС‘С‚ РґСЂСѓРіРёРј",
                        kind.label(),
                        human_minutes(penalty)
                    ));
                }
                // The route answered a moment ago, so the refusal is about the
                // request or about a connection the client made without us -
                // not about the road. Leaving its sockets alone is the point:
                // an answer arrives as a stream, and cutting them is how a user
                // loses one halfway through.
                routes::Blamed::Proven(ago) => {
                    log_proxy(&format!(
                        "РјР°СЂС€СЂСѓС‚ В«{}В» РЅС‘СЃ region-400, РЅРѕ РѕС‚РІРµС‚ РјРѕРґРµР»Рё Р±С‹Р» {} СЃ РЅР°Р·Р°Рґ вЂ” РѕСЃС‚Р°РІР»РµРЅ РєР°Рє РµСЃС‚СЊ",
                        kind.label(),
                        ago.as_secs()
                    ));
                    acted.push(format!(
                        "РїРѕ В«{}В» РјРѕРґРµР»СЊ РѕС‚РІРµС‡Р°Р»Р° {} СЃ РЅР°Р·Р°Рґ вЂ” РјР°СЂС€СЂСѓС‚ Рё РµРіРѕ СЃРѕРµРґРёРЅРµРЅРёСЏ РЅРµ С‚СЂРѕРЅСѓС‚С‹",
                        kind.label(),
                        ago.as_secs()
                    ));
                }
                routes::Blamed::AlreadyBenched => {
                    acted.push(format!(
                        "СЃРѕРµРґРёРЅРµРЅРёСЏ С‡РµСЂРµР· В«{}В» Р·Р°РєСЂС‹С‚С‹ вЂ” РїРѕРІС‚РѕСЂ РїРѕР№РґС‘С‚ РґСЂСѓРіРёРј РїСѓС‚С‘Рј",
                        kind.label()
                    ));
                }
                // A tunnel that carried an answer is still open on this route,
                // so the answer is still coming through it. The route is not
                // touched at all, and nothing is on a clock: it ends when the
                // client closes (D32).
                routes::Blamed::Streaming { idle, to_client } => {
                    log_proxy(&format!(
                        "РјР°СЂС€СЂСѓС‚ В«{}В» РЅС‘СЃ region-400, РЅРѕ РїРѕ РЅРµРјСѓ РёРґС‘С‚ РѕС‚РІРµС‚ РјРѕРґРµР»Рё ({} РљР‘, Р±Р°Р№С‚С‹ {} СЃ РЅР°Р·Р°Рґ) вЂ” РЅРµ С‚СЂРѕРЅСѓС‚",
                        kind.label(),
                        to_client / 1024,
                        idle.as_secs()
                    ));
                    // The route is protected, and so is the connection carrying
                    // the answer. The connection the refusal actually arrived
                    // on is protected only when it is that same one. For a
                    // sibling D26 still holds, narrowed to it alone: closing it
                    // makes the client dial again, and the new connection is
                    // offered the table afresh, while the answer in flight
                    // never notices.
                    let same = shape.is_some_and(|s| s.answered_ago.is_some());
                    if same && shape.is_some_and(|s| s.idle < Duration::from_millis(1500)) {
                        acted.push(format!(
                            "РїРѕ В«{}В» РїСЂСЏРјРѕ СЃРµР№С‡Р°СЃ РёРґС‘С‚ РѕС‚РІРµС‚ РјРѕРґРµР»Рё вЂ” СЃРѕРµРґРёРЅРµРЅРёРµ #{} РЅРµ С‚СЂРѕРЅСѓС‚Рѕ",
                            kind.label(),
                            tunnel
                        ));
                    } else if routes::cut_refused_tunnel(tunnel) {
                        log_proxy(&format!(
                            "СЃРѕРµРґРёРЅРµРЅРёРµ #{} В«{}В» Р·Р°РєСЂС‹С‚Рѕ РёР·-Р·Р° РѕС€РёР±РєРё 400 вЂ” РїРѕРІС‚РѕСЂ РїРѕР№РґС‘С‚ С‡РёСЃС‚С‹Рј РјР°СЂС€СЂСѓС‚РѕРј",
                            tunnel,
                            kind.label()
                        ));
                        acted.push(format!(
                            "РѕС‚РєР°Р·Р°РІС€РµРµ СЃРѕРµРґРёРЅРµРЅРёРµ #{} РїРѕ В«{}В» Р·Р°РєСЂС‹С‚Рѕ, СЃР»РµРґСѓСЋС‰РёР№ Р·Р°РїСЂРѕСЃ РїРѕР№РґС‘С‚ С‡РёСЃС‚С‹Рј РјР°СЂС€СЂСѓС‚РѕРј",
                            tunnel,
                            kind.label()
                        ));
                    } else {
                        acted.push(format!(
                            "РїРѕ В«{}В» СЃРµР№С‡Р°СЃ РёРґС‘С‚ РѕС‚РІРµС‚ РјРѕРґРµР»Рё вЂ” РјР°СЂС€СЂСѓС‚ Рё РµРіРѕ СЃРѕРµРґРёРЅРµРЅРёСЏ РЅРµ С‚СЂРѕРЅСѓС‚С‹",
                            kind.label()
                        ));
                    }
                }
            }
        }
        // The client reached Google without us, from inside a tunnel the relay
        // was standing down for: that tunnel exits somewhere blocked, so the
        // substituted address is sent back through it for a while (D15's rule,
        // alive where the loopback door is down).
        None if resolvers::vpn_is_active() => {
            resolvers::force_substitution(FORCE_SUBSTITUTE_FOR);
            log("VPN-РІС‹С…РѕРґ РЅРµ СЃРЅРёРјР°РµС‚ РіРµР№С‚ вЂ” РїРѕРґРјРµРЅР° РІРєР»СЋС‡РµРЅР° РїСЂРёРЅСѓРґРёС‚РµР»СЊРЅРѕ РЅР° 30 РјРёРЅ");
            acted.push(
                "РІР°С€ VPN РЅРµ СЃРЅРёРјР°РµС‚ Р±Р»РѕРєРёСЂРѕРІРєСѓ вЂ” РїРѕРґРјРµРЅР° Р°РґСЂРµСЃРѕРІ РІРєР»СЋС‡РµРЅР° РїРѕРІРµСЂС… С‚СѓРЅРЅРµР»СЏ РЅР° 30 РјРёРЅ"
                    .to_string(),
            );
        }
        None => {
            // The client reached Google without us. The only lever that reaches
            // it is what the name resolves to, so that is re-raced now.
            resolvers::forget_names(dns::core_namespaces());
            resolvers::warm(dns::core_namespaces(), isp_interface());
            log("Antigravity РѕР±СЂР°С‚РёР»СЃСЏ Рє Google РјРёРјРѕ РѕР±С…РѕРґР° вЂ” Р°РґСЂРµСЃР° РїРѕРґР±РёСЂР°СЋС‚СЃСЏ Р·Р°РЅРѕРІРѕ");
            acted.push(
                "Antigravity РѕР±СЂР°С‚РёР»СЃСЏ Рє Google РјРёРјРѕ РѕР±С…РѕРґР° вЂ” Р°РґСЂРµСЃР° РїРѕРґР±РёСЂР°СЋС‚СЃСЏ Р·Р°РЅРѕРІРѕ"
                    .to_string(),
            );
        }
    }
    gate::record_episode(
        count as u32,
        acted.join("; "),
        carried.map(|(k, _)| k.label()),
    );
}

fn on_answers(count: usize, at: Instant, host: Option<&str>) {
    let carried = routes::attribute_tunnel(at, host);
    if let Some((kind, tunnel)) = carried {
        routes::credit(kind);
        // The tunnel as well as the route: while *it* is open the answer is
        // still arriving, and nothing is allowed to bench or cut it (D32).
        routes::credit_tunnel(tunnel);
        // A model answer is the one thing that settles whether the relay works,
        // so it clears the relay's dud streak and lifts any bench: a client fans
        // out several gate tunnels and the short ancillary ones the relay closes
        // early otherwise read as an outage and park a working relay on the slow
        // route (no-op in a build without the relay module).
        if kind == routes::Kind::Relay {
            crate::proxy::note_relay_answer();
        }
    }
    // One line a minute at most: a user in a long session produces an answer
    // every few seconds, and the log is 64 KB. What is held back is *counted*,
    // not dropped - a report showing one answer a minute while the user was
    // getting a dozen made the refusals look like the whole story - and a
    // change of route always prints, because that is the event worth seeing.
    struct AnswerLine {
        at: Option<Instant>,
        held: usize,
        route: Option<&'static str>,
    }
    static LAST_LINE: Mutex<AnswerLine> = Mutex::new(AnswerLine {
        at: None,
        held: 0,
        route: None,
    });
    let label = carried.map_or("РЅРµ РЅР°С€", |(k, _)| k.label());
    let mut lines: Vec<(usize, &'static str)> = Vec::new();
    if let Ok(mut g) = LAST_LINE.lock() {
        // A change of route ends the old line first: what was held back came
        // through the route it came through, and printing it under the new one
        // would credit a road that carried none of it.
        if let Some(prev) = g.route.filter(|prev| *prev != label) {
            if g.held > 0 {
                lines.push((std::mem::take(&mut g.held), prev));
            }
            g.at = None;
        }
        g.held += count;
        if g.at.is_none_or(|t| t.elapsed() >= Duration::from_secs(60)) {
            g.at = Some(Instant::now());
            lines.push((std::mem::take(&mut g.held), label));
        }
        g.route = Some(label);
    }
    for (held, route) in lines {
        log_proxy(&format!(
            "РјРѕРґРµР»СЊ РѕС‚РІРµС‚РёР»Р° (x{}) вЂ” РјР°СЂС€СЂСѓС‚ В«{}В»",
            held, route
        ));
    }
    gate::record_answer(carried.map(|(k, _)| k.label()));
}

/// How long one refusal through a tunnel the relay stood down for keeps
/// substitution forced on (D15, the fallback D25 leaves for a closed door).
const FORCE_SUBSTITUTE_FOR: Duration = Duration::from_secs(30 * 60);

/// В«10 РјРёРЅВ», В«1 С‡В», В«6 С‡В».
fn human_minutes(d: Duration) -> String {
    let mins = d.as_secs() / 60;
    if mins >= 60 && mins.is_multiple_of(60) {
        format!("{} С‡", mins / 60)
    } else {
        format!("{} РјРёРЅ", mins)
    }
}

/// How long the proxy listener is given to appear before the route is treated as
/// absent. Covers `proxy::bind_listener`'s own retries (15 s) with room to spare.
const PROXY_START_BUDGET: Duration = Duration::from_secs(30);

/// Runs until killed, and never returns.
///
/// The DNS listener is started **last and separately** (`serve_dns_forever`),
/// and a port it cannot take is no longer the end of the process. It used to
/// be: `run` bound `127.0.0.53:53` first and returned the error, `main` logged
/// it as fatal and exited 1. A field report (2026-09-20) showed what that costs
/// - something on that machine had held UDP `:53` for a day, so every start
///   died within seconds, and with it went the gate hosts' door, the local proxy,
///   the built-in exits, the route table, the region-400 watch and the auto-patch
///   watchdog. Not one of those needs port 53. That user had a working bypass
///   available the whole time and got nothing.
pub fn run() -> Result<(), String> {
    // Detect once up front. Otherwise the first query pays for a cold probe,
    // which is long enough that Windows gives up on us and falls back to the
    // direct resolvers - and then caches that unsubstituted answer for its full
    // TTL, so one slow startup is felt for minutes.
    log(&format!("egress: if{}", isp_interface()));
    record_version();
    // Before any listener is tried: from here on the window can tell this
    // process from its own watchdog by the age of what it writes (P54).
    gate::note_started();
    thread::spawn(warm_forever);
    // The proxy variable is user-wide and must never outlive the listener it
    // names, so the watchdog takes it off when the listener is dead for a while
    // (G20). This is the other half: the listener is up, so the route is back.
    // Off the startup path - it is a PowerShell call - and never in front of a
    // proxy the user set themselves.
    //
    // "Is up", not "is coming up": this used to run the moment the relay started,
    // which is a promise about a socket that had not been bound yet. When the bind
    // then failed - the port is inside Windows' dynamic range, so it can be taken
    // (G31) - the variable was restored anyway and the watchdog took it back off
    // ninety seconds later, every logon, and everything proxy-aware on the machine
    // spent that window with no network.
    spawn_proxy_env_maintainer();

    // The fallback route lives in this process because it needs the same two
    // things the relay already has: the ISP interface, and the resolver pool
    // that knows which provider is substituting right now. It only ever carries
    // traffic that is actually pointed at it, so starting it here costs a
    // listening socket and nothing else.
    let egress = isp_interface();
    thread::spawn(move || {
        if let Err(e) = proxy::run(egress) {
            log_proxy(&format!("not started: {}", e));
        }
    });
    // The gate hosts' own door (`loopback`). If it cannot be bound the relay
    // keeps answering with substituted addresses, exactly as before.
    thread::spawn(|| {
        if let Err(e) = loopback::run() {
            log_proxy(&format!("Р»РѕРєР°Р»СЊРЅС‹Рµ Р°РґСЂРµСЃР° РіРµР№С‚-С…РѕСЃС‚РѕРІ РЅРµ Р·Р°РЅСЏС‚С‹: {}", e));
        }
    });
    thread::spawn(watch_client_logs);

    serve_dns_forever()
}

/// The Linux service entry point: runs the local CONNECT proxy route as a
/// background service without a port-53 DNS listener.
///
/// It maintains gate.json, the resolver pool, client log watching, and the
/// proxy environment variable, then serves the local proxy on 127.0.0.1:53129.
pub fn run_proxy_service() -> Result<(), String> {
    log_proxy("СЃР»СѓР¶Р±Р° РѕР±С…РѕРґР° (Р»РѕРєР°Р»СЊРЅС‹Р№ РїСЂРѕРєСЃРё) Р·Р°РїСѓС‰РµРЅР°");
    record_version();
    gate::note_started();
    thread::spawn(warm_forever);
    thread::spawn(watch_client_logs);
    spawn_proxy_env_maintainer();
    proxy::run(0)
}

fn spawn_proxy_env_maintainer() {
    thread::spawn(|| {
        let var = crate::endpoint::PROXY_ENV_VAR;
        // Before the wait, and unconditionally: retiring the user-wide pair an
        // older build wrote has nothing to do with whether *our* listener came
        // up. Behind the wait it would be skipped on exactly the machines that
        // need it most - the ones where 53129 is taken or reserved, where the old
        // value names a port that no longer answers and takes the whole machine's
        // proxy-aware traffic with it (G20, G31).
        match crate::endpoint::remove_legacy_proxy_env(&proxy::proxy_url()) {
            Ok(true) => log_proxy("СЃРЅСЏС‚Р° РїСЂРµР¶РЅСЏСЏ РѕР±С‰РµСЃРёСЃС‚РµРјРЅР°СЏ HTTPS_PROXY"),
            Ok(false) => {}
            Err(e) => log_proxy(&format!("РїСЂРµР¶РЅСЏСЏ HTTPS_PROXY РЅРµ СЃРЅСЏС‚Р°: {}", e)),
        }
        let mut waiting_said = false;
        loop {
            // The port that answered is the one named: the proxy may have moved
            // while this waited (P26), and only our own listener counts - not a
            // program of someone else's answering on the default port.
            let Some(port) = proxy::wait_for_our_listener(PROXY_START_BUDGET) else {
                // It keeps trying (`proxy::run`, P53): an antivirus exception
                // or a closed program frees the port without a restart, and the
                // variable follows the listener up - still only once it answers.
                // Said once per outage, not once a minute: this log is 64 KB.
                if !waiting_said {
                    log_proxy(&format!(
                        "{} РЅРµ РІС‹СЃС‚Р°РІР»РµРЅР°: Р»РѕРєР°Р»СЊРЅС‹Р№ РїСЂРѕРєСЃРё РЅРµ РїРѕРґРЅСЏР»СЃСЏ",
                        var
                    ));
                    waiting_said = true;
                }
                continue;
            };
            waiting_said = false;
            let url = proxy::url_at(port);
            if crate::settings::local_proxy_wanted() {
                // `ensure_proxy_env` asks `foreign_proxy` first, so a proxy the
                // user set themselves still outranks ours (D19, I52, G33).
                match crate::endpoint::ensure_proxy_env(&url) {
                    Ok(crate::endpoint::Outcome::Applied) => {
                        log_proxy(&format!("{} СЃРЅРѕРІР° СѓРєР°Р·С‹РІР°РµС‚ РЅР° Р»РѕРєР°Р»СЊРЅС‹Р№ РїСЂРѕРєСЃРё", var))
                    }
                    Ok(crate::endpoint::Outcome::AlreadySet) => {}
                    Err(e) => log_proxy(&format!("{} РЅРµ РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅР°: {}", var, e)),
                }
            } else if crate::endpoint::proxy_env_is_ours() {
                // Off in the settings and still set. `remove_proxy` judges each
                // variable on its own: `PROXY_ENV_VAR` is a name only this tool
                // writes, and the legacy pair is removed by value, so a proxy of
                // the user's own is never taken off here.
                match crate::endpoint::remove_proxy(&url, "") {
                    Ok(()) => log_proxy(&format!(
                        "{} СЃРЅСЏС‚Р°: Р»РѕРєР°Р»СЊРЅС‹Р№ РїСЂРѕРєСЃРё РІС‹РєР»СЋС‡РµРЅ РІ РЅР°СЃС‚СЂРѕР№РєР°С…",
                        var
                    )),
                    Err(e) => log_proxy(&format!("{} РЅРµ СЃРЅСЏС‚Р°: {}", var, e)),
                }
            }
            thread::sleep(proxy::REBIND_EVERY);
        }
    });
}

/// Where the NRPT rules send their queries, as an address to bind and to
/// diagnose. `LISTEN_IP` is a literal and always parses; the fallback only
/// exists so a typo in it could never panic the relay.
fn listen_addr() -> SocketAddr {
    SocketAddr::from((
        LISTEN_IP.parse::<Ipv4Addr>().unwrap_or(Ipv4Addr::LOCALHOST),
        LISTEN_PORT,
    ))
}

/// How many `recv_from` failures in a row mean the socket is gone rather than
/// one exchange having gone wrong.
///
/// Windows surfaces an ICMP port-unreachable for an answer we already sent as
/// `WSAECONNRESET` on the *next* receive, so single errors are ordinary and are
/// skipped, as they always were. Sixty-four of them with not one query in
/// between is a socket worth dropping and taking again.
const RECV_ERRORS_BEFORE_REBIND: u32 = 64;

/// Holds `127.0.0.53:53` and answers on it, for as long as the process lives.
///
/// A port that cannot be had is diagnosed (`portcheck`), published for the
/// window's card (`gate::set_blocker`) and tried again every minute, the same
/// way the local proxy and the door already do. Unlike those two it cannot move
/// to another port: the NRPT rules name an address and Windows sends DNS to
/// port 53 or nowhere. Meanwhile the NRPT rules list this address *first* and
/// the substituting providers after it, so a Windows client falls through to
/// them on its own - the layer degrades to slow rather than to nothing.
fn serve_dns_forever() -> ! {
    let addr = listen_addr();
    let mut said: Option<crate::gate::Blocker> = None;
    loop {
        let sock = match UdpSocket::bind(addr) {
            Ok(sock) => {
                if said.take().is_some() {
                    log("РїРѕСЂС‚ DNS-СЂРµР»РµСЏ РѕСЃРІРѕР±РѕРґРёР»СЃСЏ");
                    gate::set_blocker("dns", None);
                }
                sock
            }
            Err(e) => {
                let blocker = crate::portcheck::diagnose_on(addr, &e, crate::portcheck::Proto::Udp)
                    .blocker("dns", addr, &e);
                // Once per distinct cause: the retry is a minute, the log is 64 KB.
                if said.as_ref() != Some(&blocker) {
                    log(&format!(
                        "РЅРµ Р·Р°РЅСЏС‚СЊ {} вЂ” {} ({}{})",
                        blocker.addr,
                        blocker.error,
                        blocker.cause,
                        if blocker.by.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", blocker.by)
                        }
                    ));
                    said = Some(blocker.clone());
                }
                // Every try, so the record never goes stale under the card.
                gate::set_blocker("dns", Some(blocker));
                thread::sleep(proxy::REBIND_EVERY);
                continue;
            }
        };
        log(&format!("start: {}:{}", LISTEN_IP, LISTEN_PORT));
        // A client that was running before us is on answers that predate our
        // rules, and nothing else tells it otherwise (G75). Here and not a line
        // earlier: the socket is bound, so a re-ask reaches us.
        if crate::dns::flush_client_cache() {
            log("РєСЌС€ DNS-РєР»РёРµРЅС‚Р° РїРѕ РіРµР№С‚-РёРјРµРЅР°Рј СЃР±СЂРѕС€РµРЅ вЂ” РєР»РёРµРЅС‚ СЃРїСЂРѕСЃРёС‚ Р·Р°РЅРѕРІРѕ");
        }
        let up = Instant::now();
        serve_queries(&sock);
        drop(sock);
        log("DNS-СЃРѕРєРµС‚ РїРµСЂРµСЃС‚Р°Р» РїСЂРёРЅРёРјР°С‚СЊ Р·Р°РїСЂРѕСЃС‹ вЂ” Р·Р°РЅРёРјР°РµРј Р·Р°РЅРѕРІРѕ");
        // Never in a tight loop: a socket that did not last a minute is not one
        // an immediate retry will fix, so it waits out the same clock a failed
        // bind does.
        if up.elapsed() < proxy::REBIND_EVERY {
            thread::sleep(proxy::REBIND_EVERY);
        }
    }
}

/// Answers queries on `sock` until it stops taking them.
fn serve_queries(sock: &UdpSocket) {
    let mut buf = [0u8; 4096];
    let mut misses = 0u32;
    loop {
        let (n, from) = match sock.recv_from(&mut buf) {
            Ok(v) => {
                misses = 0;
                v
            }
            Err(_) => {
                misses += 1;
                if misses >= RECV_ERRORS_BEFORE_REBIND {
                    return;
                }
                continue;
            }
        };
        // Anything shorter than a header is not a query worth relaying.
        if n < 12 {
            continue;
        }
        let query = buf[..n].to_vec();
        let out = match sock.try_clone() {
            Ok(s) => s,
            Err(_) => continue,
        };
        // One thread per query: the volume is a handful per minute, and a slow
        // upstream must not stall the queries behind it.
        thread::spawn(move || {
            let name = dns_client::question_name(&query).unwrap_or_else(|| "?".to_string());
            if let Some(reply) = own_answer(&query, &name) {
                out.send_to(&reply, from).ok();
                return;
            }
            match relay(&query) {
                Some((reply, provider, verdict)) => {
                    out.send_to(&reply, from).ok();
                    // The verdict is the part worth logging: "ok" used to mean
                    // only that bytes came back, which is precisely what it
                    // still said while the answers had stopped being
                    // substituted.
                    log(&format!(
                        "{:<12} {} [{}]",
                        verdict_tag(verdict),
                        name,
                        provider
                    ));
                }
                None => log(&format!("fail         {}", name)),
            }
        });
    }
}

/// The answer the relay gives a gate host itself instead of relaying anyone's.
///
/// A record: the loopback listener's address for that host, while it is up
/// (`loopback`). AAAA, SVCB and HTTPS: an empty answer, always - every provider
/// answers AAAA with Google's own IPv6 or with nothing, and an IPv6 client
/// handed the former dials the gate from the blocked region with the whole
/// layer "working" (P40); an HTTPS record can carry address hints of its own.
/// Anything else is relayed as before.
fn own_answer(query: &[u8], name: &str) -> Option<Vec<u8>> {
    if !proxy::is_gate_host(name) {
        return None;
    }
    match dns_client::question_type(query)? {
        1 => {
            let ip = loopback::answer_for(name)?;
            note_loopback(name);
            dns_client::synth_reply(query, Some(ip), loopback::ANSWER_TTL)
        }
        28 | 64 | 65 => dns_client::synth_reply(query, None, loopback::ANSWER_TTL),
        _ => None,
    }
}

/// Logs that a gate host is being answered with our own address - once, and
/// again only after it has not been for a while, so a client re-asking every
/// thirty seconds does not fill the 64 KB log.
fn note_loopback(name: &str) {
    static LAST: Mutex<Option<std::collections::HashMap<String, Instant>>> = Mutex::new(None);
    let due = LAST
        .lock()
        .map(|mut g| {
            let map = g.get_or_insert_with(std::collections::HashMap::new);
            let key = name.to_ascii_lowercase();
            let due = map
                .get(&key)
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(30 * 60));
            if due {
                map.insert(key, Instant::now());
            }
            due
        })
        .unwrap_or(false);
    if due {
        log(&format!("loopback     {} [РЅР°С€ РјР°СЂС€СЂСѓС‚]", name));
    }
}


#[cfg(test)]
mod tests;

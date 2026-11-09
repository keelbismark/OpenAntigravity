//! Reads the two facts no probe can measure - the region 400, and a model
//! answer - out of the language server's own log.
//!
//! `cloudcode-pa` refuses a blocked region with `FAILED_PRECONDITION (code 400):
//! User location is not supported for the API use`. That answer travels inside
//! the client's own TLS, and an unauthenticated probe never sees it -
//! `loadCodeAssist` answers 401 from a permitted and a blocked exit alike
//! (kb/dns.md). Every signal the tool has about a route's region is therefore
//! indirect: the exit's country, whether an address was substituted, whether a
//! tunnel carried bytes. The client's log is the only place the refusal is
//! written down in plain text, so the relay tails it.
//!
//! The other half is the one thing that proves the bypass works: a model answer
//! logs `вЂ¦/v1internal:streamGenerateContent?alt=sse Trace: 0xвЂ¦ ResponseID: вЂ¦`
//! when it starts streaming, and a refused turn logs no such line (measured on
//! the IDE log of 2026-09-12: refusals 14:58вЂ“15:00, first `ResponseID` 15:16).
//! Silence proves nothing - a user who gave up is silent too (G50) - so the
//! window says "СЂР°Р±РѕС‚Р°РµС‚" only on this line, and the route table ranks routes by
//! it (`routes`).
//!
//! Tailing, not reading: each pass looks at the lines appended since the last
//! one and nothing else, so an old refusal from a session hours ago cannot
//! trigger anything now. A file seen for the first time is taken from its end
//! for the same reason. A file that shrank was rotated or the app restarted, and
//! is read from the start - it is new content either way.
//!
//! Two readers, asking different questions of the same file. `poll` is the
//! relay's: "what came in since I last looked", which is what may be *acted*
//! on. `newest_refusal` is the window's: "is the user hitting the gate right
//! now", which has to be answerable about the minutes before this process
//! started - so it reads the log's own stamps rather than a saved offset. The
//! two share the pattern and the file list and nothing else; they run in
//! different processes and neither moves the other's position.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// The refusal, as the language server logs it. Matched ASCII-case-insensitively
/// and never on the whole `FAILED_PRECONDITION` line: that status wraps other
/// preconditions too, and only this one is about where the request came from.
const REGION_400: &str = "user location is not supported";

/// A model answer, as the language server logs it: the generation call and the
/// id of the response it got back, on one line. Both halves: `loadCodeAssist`
/// and `fetchAvailableModels` are logged the same way on a refused session too,
/// and only the generation call is behind the gate.
const ANSWER_CALL: &str = "streamgeneratecontent";
const ANSWER_ID: &str = "responseid:";

/// Longest slice of appended log read in one pass. Anything beyond it is a log
/// that grew by megabytes between two passes, which is not a session anyone is
/// working in; the tail is what carries the news.
const MAX_READ: u64 = 1024 * 1024;

/// How many products the scan is willing to look at. A user profile does not
/// hold more than a handful of `Antigravity*` folders, and a bound keeps a
/// pathological `%APPDATA%` from turning a 15 s tick into a directory walk.
const MAX_PRODUCTS: usize = 8;

/// Where each watched file was read up to.
static OFFSETS: Mutex<Option<HashMap<PathBuf, u64>>> = Mutex::new(None);

/// The gate host each file last named in a `URL:` line, carried across passes:
/// a refusal line names no host, and the call it refused was logged in an
/// earlier chunk more often than not.
static LAST_HOST: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);

/// Whether the first pass has run. Files found on it hold history and are taken
/// from their end; a file that *appears* later is a new session - an IDE launch,
/// a CLI run - and everything in it is news, from the first byte. Taking those
/// from their end too lost the answer (or the refusal) a short CLI run writes
/// before the next pass comes round.
static FIRST_PASS_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What one stretch of log said about the gate.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tally {
    /// Refusal lines. One refused turn writes several (the stream handler, the
    /// executor and the error report each repeat it), so this counts lines, not
    /// turns.
    pub refusals: usize,
    /// How long ago the newest refusal was stamped. `None` when there was none,
    /// or its stamp could not be placed against the clock.
    pub refused_ago: Option<Duration>,
    /// Model answers: one line per answer that started streaming.
    pub answers: usize,
    pub answered_ago: Option<Duration>,
    /// Which gate host the newest refusal was about: the host of the last
    /// `URL: https://вЂ¦` line before it in this file. The client keeps a
    /// connection pool per host, and the two can sit on different routes.
    pub refused_host: Option<String>,
    /// The host the newest answer's own line names.
    pub answered_host: Option<String>,
}

impl Tally {
    pub fn is_empty(&self) -> bool {
        self.refusals == 0 && self.answers == 0
    }

    fn add(&mut self, other: Tally) {
        self.refusals += other.refusals;
        self.answers += other.answers;
        // The host goes with whichever event is newer - an unplaceable one
        // (no age) never displaces a placed one.
        if (other.refused_ago.is_some() && newer(self.refused_ago, other.refused_ago) == other.refused_ago)
            || self.refused_host.is_none()
        {
            self.refused_host = other.refused_host.clone();
        }
        if (other.answered_ago.is_some() && newer(self.answered_ago, other.answered_ago) == other.answered_ago)
            || self.answered_host.is_none()
        {
            self.answered_host = other.answered_host.clone();
        }
        self.refused_ago = newer(self.refused_ago, other.refused_ago);
        self.answered_ago = newer(self.answered_ago, other.answered_ago);
    }
}

fn newer(a: Option<Duration>, b: Option<Duration>) -> Option<Duration> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// One pass over every client log on the machine. Returns the files that
/// gained a refusal or an answer since the last pass, with what each gained.
pub fn poll() -> Vec<(PathBuf, Tally)> {
    let mut out = Vec::new();
    let Ok(mut guard) = OFFSETS.lock() else {
        return out;
    };
    let offsets = guard.get_or_insert_with(HashMap::new);
    let now = crate::utils::local_clock();
    let started = FIRST_PASS_DONE.swap(true, std::sync::atomic::Ordering::Relaxed);
    let mut hosts = LAST_HOST.lock().ok();
    for path in candidate_logs() {
        let first_sight = !offsets.contains_key(&path);
        // A file new since the first pass is read from its start (see above).
        let from = match offsets.get(&path).copied() {
            None if started => Some(0),
            other => other,
        };
        let mut last_host = hosts
            .as_mut()
            .and_then(|h| h.get_or_insert_with(HashMap::new).get(&path).cloned());
        let (next, tally) = scan(&path, from, now, &mut last_host);
        if let (Some(h), Some(host)) = (hosts.as_mut(), last_host) {
            h.get_or_insert_with(HashMap::new).insert(path.clone(), host);
        }
        offsets.insert(path.clone(), next);
        if (!first_sight || started) && !tally.is_empty() {
            out.push((path, tally));
        }
    }
    out
}

/// Reads `path` from `from` (or from its end, when it has never been read) and
/// tallies the complete lines appended since. Returns where the next read
/// starts: just past the last complete line, so a line the logger's buffer
/// flush split across two passes is read once, whole, on the second.
///
/// Pure in the sense that matters for a test: no static, one file, one answer.
fn scan(
    path: &Path,
    from: Option<u64>,
    now: Option<crate::utils::LocalClock>,
    last_host: &mut Option<String>,
) -> (u64, Tally) {
    let Ok(len) = fs::metadata(path).map(|m| m.len()) else {
        return (from.unwrap_or(0), Tally::default());
    };
    let Some(from) = from else {
        // Never seen: history is not news.
        return (len, Tally::default());
    };
    // Shrunk means rotated or restarted, and everything in it is new.
    let mut start = if len < from { 0 } else { from };
    if len == start {
        return (len, Tally::default());
    }
    if len - start > MAX_READ {
        start = len - MAX_READ;
    }
    let Ok(mut f) = File::open(path) else {
        return (from, Tally::default());
    };
    if f.seek(SeekFrom::Start(start)).is_err() {
        return (from, Tally::default());
    }
    let mut buf = Vec::with_capacity((len - start) as usize);
    if f.take(len - start).read_to_end(&mut buf).is_err() {
        return (from, Tally::default());
    }
    let Some(last_nl) = buf.iter().rposition(|b| *b == b'\n') else {
        // No complete line yet. Wait for the rest - unless the slice is the
        // whole read budget, which is one line nobody will ever finish.
        if buf.len() as u64 >= MAX_READ {
            return (len, Tally::default());
        }
        return (start, Tally::default());
    };
    let complete = &buf[..=last_nl];
    (start + last_nl as u64 + 1, tally(complete, now, last_host))
}

/// Refusals and answers in a stretch of complete lines, with the newest of each
/// placed against `now` by the line's own stamp.
fn tally(
    chunk: &[u8],
    now: Option<crate::utils::LocalClock>,
    last_host: &mut Option<String>,
) -> Tally {
    let mut out = Tally::default();
    for line in chunk.split(|b| *b == b'\n') {
        let named = url_host(line);
        if named.is_some() {
            *last_host = named.clone();
        }
        let refusals = count_refusals(line);
        let answered = is_answer(line);
        if refusals == 0 && !answered {
            continue;
        }
        let ago = now.and_then(|now| {
            let text = String::from_utf8_lossy(line);
            parse_stamp(&text)
                .and_then(|at| age_secs(at, now))
                .map(|s| Duration::from_secs(s as u64))
        });
        if refusals > 0 {
            out.add(Tally {
                refusals,
                refused_ago: ago,
                refused_host: last_host.clone(),
                ..Tally::default()
            });
        }
        if answered {
            out.add(Tally {
                answers: 1,
                answered_ago: ago,
                answered_host: named.or_else(|| last_host.clone()),
                ..Tally::default()
            });
        }
    }
    out
}

/// The host of a `URL: https://<host>/вЂ¦` line, lower-cased; `None` for any
/// other line or a host that is not one of ours.
fn url_host(line: &[u8]) -> Option<String> {
    const MARK: &[u8] = b"URL: https://";
    let at = line.windows(MARK.len()).position(|w| w == MARK)? + MARK.len();
    let rest = &line[at..];
    let end = rest
        .iter()
        .position(|b| *b == b'/' || *b == b':' || b.is_ascii_whitespace())
        .unwrap_or(rest.len());
    let host = String::from_utf8_lossy(&rest[..end]).to_ascii_lowercase();
    crate::proxy::is_gate_host(&host).then_some(host)
}

/// Whether a line is a model answer that started streaming.
fn is_answer(line: &[u8]) -> bool {
    contains_ci(line, ANSWER_CALL.as_bytes()) && contains_ci(line, ANSWER_ID.as_bytes())
}

fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    hay.len() >= needle.len() && hay.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

/// Occurrences of the refusal in a chunk of log, case-insensitively. Byte-wise,
/// because the log is a stream of glog lines that a partial last line may cut
/// anywhere, and a UTF-8 boundary is not something worth failing on.
fn count_refusals(buf: &[u8]) -> usize {
    let needle = REGION_400.as_bytes();
    if buf.len() < needle.len() {
        return 0;
    }
    buf.windows(needle.len())
        .filter(|w| w.eq_ignore_ascii_case(needle))
        .count()
}

/// The language-server logs of every Antigravity product under the user's
/// roaming profile, and the CLI's.
///
/// Desktop writes `<product>\logs\language_server.log`; the IDE, a VS Code
/// fork, writes `<product>\logs\<session stamp>\ls-main.log`, one folder per
/// launch - so for it only the newest session is watched. Product folders are
/// matched by prefix rather than listed, so a rebranded build is picked up too.
/// The CLI (`agy`) writes `~\.gemini\antigravity-cli\log\cli-<stamp>.log`, one
/// per session, in the same glog format - found 2026-09-18, and watched from
/// 2.14.0_1 on (before that "CLI log location unknown").
/// Public because the window holds the list rather than rebuilding it: this
/// walks `%APPDATA%` and then the IDE's `logs` folder, which has one sub-folder
/// per launch, and a window ticking every three seconds must not do that.
pub fn candidate_logs() -> Vec<PathBuf> {
    let Some(root) = profile_root() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut products = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.to_ascii_lowercase().starts_with("antigravity") {
            continue;
        }
        products += 1;
        if products > MAX_PRODUCTS {
            break;
        }
        let logs = entry.path().join("logs");
        let desktop = logs.join("language_server.log");
        if desktop.is_file() {
            out.push(desktop);
        }
        if let Some(session) = newest_session(&logs) {
            let ide = session.join("ls-main.log");
            if ide.is_file() {
                out.push(ide);
            }
        }
    }
    if let Some(cli) = newest_cli_log() {
        out.push(cli);
    }
    out
}

/// The CLI's current session log: the newest `cli-*.log`.
fn newest_cli_log() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let dir = PathBuf::from(home)
        .join(".gemini")
        .join("antigravity-cli")
        .join("log");
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with("cli-") && n.ends_with(".log")
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by(|(ma, pa), (mb, pb)| ma.cmp(mb).then_with(|| pa.cmp(pb)))
        .map(|(_, p)| p)
}

/// The most recently modified sub-folder of `logs`, i.e. the IDE's current
/// session.
fn newest_session(logs: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(logs).ok()?;
    entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, e.path()))
        })
        // Ties - same second on a coarse filesystem - go to the later stamp:
        // the folder names are timestamps and sort chronologically.
        .max_by(|(ma, pa), (mb, pb)| ma.cmp(mb).then_with(|| pa.cmp(pb)))
        .map(|(_, p)| p)
}

#[cfg(target_os = "windows")]
fn profile_root() -> Option<PathBuf> {
    std::env::var("APPDATA").ok().map(PathBuf::from)
}

/// Linux keeps the same layout under `~/.config`; nothing tails it there yet
/// because the DNS layer it would steer is not ported, but the path is right.
#[cfg(not(target_os = "windows"))]
fn profile_root() -> Option<PathBuf> {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(x) if !x.is_empty() => Some(PathBuf::from(x)),
        _ => std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".config")),
    }
}

// ---------------------------------------------------------------------------
// Reading the same log the other way round: what is *there*, not what is new.
// ---------------------------------------------------------------------------

/// The newest refusal a client log carries, as the window needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sighting {
    /// How long ago the newest one was written, by the log's own stamp.
    pub ago: Duration,
    /// How many fall inside the window asked for.
    pub count: usize,
}

/// How much of a log's tail is read looking for one. A session's worth of glog
/// is a few hundred kilobytes an hour, and the window asked for is minutes.
const HISTORY_TAIL: u64 = 256 * 1024;

/// A stamp a second or two ahead of our own read of the clock is *now*, not a
/// line from the future: the logger and this process read the same clock at
/// slightly different moments, and `GetLocalTime` has whole-second resolution.
const CLOCK_SLACK: u32 = 5;

/// The newest refusal in any client log inside `within`, and how many there are.
///
/// `poll` above answers "what is new since I last looked", which is the right
/// question for the relay and the wrong one for a window: a user who hits the
/// gate and *then* opens this tool would be told nothing had happened. So this
/// reads the log's own timestamps rather than a saved position, and it is the
/// only place in the tool that parses them.
///
/// `None` without a local clock to compare against (non-Windows): an age nobody
/// can compute is not one to guess at.
#[cfg_attr(not(test), allow(dead_code))]
pub fn newest_refusal(paths: &[PathBuf], within: Duration) -> Option<Sighting> {
    // Occurrences, not lines: one line can carry more than one, and
    // `Episode.count` on the relay's side counts them the same way.
    newest_matching(paths, within, |line| count_refusals(line.as_bytes()))
}

/// The newest model answer any client log carries inside `within`, and how many
/// there are. The window's proof that the bypass works right now.
#[cfg_attr(not(test), allow(dead_code))]
pub fn newest_answer(paths: &[PathBuf], within: Duration) -> Option<Sighting> {
    newest_matching(paths, within, |line| is_answer(line.as_bytes()) as usize)
}

/// Everything the window asks of the logs, in one read of each tail.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    /// Refusals inside `recent`: the newest, and how many.
    pub refused_recent: Option<Sighting>,
    /// The newest refusal inside `horizon`, however old.
    pub refused: Option<Sighting>,
    /// The newest model answer inside `horizon`, and how many.
    pub answered: Option<Sighting>,
    /// The newest account-verification demand inside `horizon`.
    pub verify: Option<Verification>,
}

/// `newest_refusal` and `newest_answer` over two windows, from a single pass:
/// the window asked three questions of the same tail every time the log grew,
/// and a busy Desktop log grows every second.
pub fn history(paths: &[PathBuf], recent: Duration, horizon: Duration) -> History {
    let Some(now) = crate::utils::local_clock() else {
        return History::default();
    };
    let mut out = History::default();
    let bump = |slot: &mut Option<Sighting>, ago: Duration, hits: usize| {
        *slot = Some(match *slot {
            Some(s) => Sighting {
                ago: s.ago.min(ago),
                count: s.count + hits,
            },
            None => Sighting { ago, count: hits },
        });
    };
    for path in paths {
        let Some(text) = tail(path, HISTORY_TAIL) else {
            continue;
        };
        for line in text.lines() {
            let refusals = count_refusals(line.as_bytes());
            let answered = is_answer(line.as_bytes());
            if refusals == 0 && !answered {
                continue;
            }
            let Some(ago) = parse_stamp(line).and_then(|at| age_secs(at, now)) else {
                continue;
            };
            let ago = Duration::from_secs(ago as u64);
            if ago > horizon {
                continue;
            }
            if refusals > 0 {
                bump(&mut out.refused, ago, refusals);
                if ago <= recent {
                    bump(&mut out.refused_recent, ago, refusals);
                }
            }
            if answered {
                bump(&mut out.answered, ago, 1);
            }
        }
        if let Some(v) = newest_verification(&text, now, horizon) {
            if out.verify.as_ref().is_none_or(|o| v.ago < o.ago) {
                out.verify = Some(v);
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStatus {
    Ok200,
    Refusal400,
    Error,
}

impl TurnStatus {
    pub fn badge(&self) -> &'static str {
        match self {
            TurnStatus::Ok200 => "200 OK",
            TurnStatus::Refusal400 => "400 BLOCKED",
            TurnStatus::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTurnEvent {
    pub time_hms: String,
    pub ago_secs: Option<u64>,
    pub model: String,
    pub method: String,
    pub status: TurnStatus,
    pub response_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActivitySummary {
    pub total_turns: usize,
    pub success_turns: usize,
    pub blocked_turns: usize,
    pub newest_event_ago: Option<Duration>,
    pub recent_events: Vec<ModelTurnEvent>,
}

fn extract_response_id(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let idx = lower.find("responseid:")?;
    let rest = line[idx + "responseid:".len()..].trim_start();
    let token: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(16)
        .collect();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn extract_model_name(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    if lower.contains("gemini-2.5-pro") {
        "Gemini 2.5 Pro".to_string()
    } else if lower.contains("gemini-2.5-flash") {
        "Gemini 2.5 Flash".to_string()
    } else if lower.contains("gemini-1.5-pro") {
        "Gemini 1.5 Pro".to_string()
    } else if lower.contains("gemini-1.5-flash") {
        "Gemini 1.5 Flash".to_string()
    } else if lower.contains("claude-3-5-sonnet") || lower.contains("claude-3.5-sonnet") {
        "Claude 3.5 Sonnet".to_string()
    } else if lower.contains("claude") {
        "Claude".to_string()
    } else if let Some(pos) = lower.find("models/") {
        let rest = &line[pos + 7..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(24)
            .collect();
        if !name.is_empty() {
            format!("models/{}", name)
        } else {
            "Gemini (Stream)".to_string()
        }
    } else if lower.contains("streamgeneratecontent") {
        "Gemini (Stream)".to_string()
    } else {
        "Gemini API".to_string()
    }
}

pub fn inspect_recent_activity(paths: &[PathBuf], max_events: usize) -> ActivitySummary {
    let now = crate::utils::local_clock();
    let mut summary = ActivitySummary::default();
    let mut events = Vec::new();

    for path in paths {
        let Some(text) = tail(path, HISTORY_TAIL) else {
            continue;
        };

        for line in text.lines() {
            let is_ans = is_answer(line.as_bytes());
            let refusals = count_refusals(line.as_bytes());
            let is_call = contains_ci(line.as_bytes(), ANSWER_CALL.as_bytes());

            if !is_ans && refusals == 0 && !is_call {
                continue;
            }

            summary.total_turns += 1;
            let status = if refusals > 0 {
                summary.blocked_turns += 1;
                TurnStatus::Refusal400
            } else if is_ans {
                summary.success_turns += 1;
                TurnStatus::Ok200
            } else {
                TurnStatus::Ok200
            };

            let stamp = parse_stamp(line);
            let time_hms = match stamp {
                Some((_, _, sec)) => {
                    let h = sec / 3600;
                    let m = (sec % 3600) / 60;
                    let s = sec % 60;
                    format!("{:02}:{:02}:{:02}", h, m, s)
                }
                None => "--:--:--".to_string(),
            };

            let ago_secs = now.and_then(|n| {
                stamp.and_then(|st| age_secs(st, n)).map(|s| s as u64)
            });

            if let Some(s) = ago_secs {
                let dur = Duration::from_secs(s);
                if summary.newest_event_ago.is_none_or(|cur| dur < cur) {
                    summary.newest_event_ago = Some(dur);
                }
            }

            let model = extract_model_name(line);
            let method = if is_call || is_ans {
                "streamGenerateContent".to_string()
            } else if line.to_ascii_lowercase().contains("fetchavailablemodels") {
                "fetchAvailableModels".to_string()
            } else {
                "loadCodeAssist".to_string()
            };
            let response_id = extract_response_id(line);

            events.push(ModelTurnEvent {
                time_hms,
                ago_secs,
                model,
                method,
                status,
                response_id,
            });
        }
    }

    // Keep the most recent events (newest first)
    events.reverse();
    if events.len() > max_events {
        events.truncate(max_events);
    }
    summary.recent_events = events;
    summary
}

fn newest_matching(
    paths: &[PathBuf],
    within: Duration,
    hits_in: impl Fn(&str) -> usize,
) -> Option<Sighting> {
    let now = crate::utils::local_clock()?;
    let mut newest: Option<Duration> = None;
    let mut count = 0usize;
    for path in paths {
        let Some(text) = tail(path, HISTORY_TAIL) else {
            continue;
        };
        for line in text.lines() {
            let hits = hits_in(line);
            if hits == 0 {
                continue;
            }
            // A line whose stamp will not parse is one the tail cut in half, or
            // one some future build writes differently. Either way it cannot be
            // placed in time, and an unplaceable refusal is not evidence of
            // anything happening *now*.
            let Some(ago) = parse_stamp(line).and_then(|at| age_secs(at, now)) else {
                continue;
            };
            let ago = Duration::from_secs(ago as u64);
            if ago > within {
                continue;
            }
            count += hits;
            if newest.is_none_or(|n| ago < n) {
                newest = Some(ago);
            }
        }
    }
    newest.map(|ago| Sighting { ago, count })
}

/// Google asking the signed-in account to prove itself before it may use the
/// model: a `403` whose `google.rpc.ErrorInfo` says `VALIDATION_REQUIRED` and
/// carries the page to do it on in `metadata.validation_url` (the same field
/// arrives as `validationUrl` on an ineligible tier at sign-in). The client is
/// meant to show В«Complete verificationВ» for it, and does not always - the
/// link is then only in its log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    /// How long ago, by the log's own stamp.
    pub ago: Duration,
    /// The page, decoded from the JSON it came in and checked to be Google's.
    pub url: String,
}

/// The reason code, as the client itself matches it (`reason ===
/// "VALIDATION_REQUIRED"` in the workbench).
const VERIFY_REASON: &[u8] = b"VALIDATION_REQUIRED";
/// How far from the link its reason may be and still be the same error body.
/// Google pretty-prints the body, and `details` holds a handful of short
/// entries; a few kilobytes covers it without reaching a neighbouring error.
const VERIFY_REACH: usize = 4096;
/// How many lines back from the link its entry's stamp is looked for: the body
/// spans lines of its own, and only the line that opens the entry is stamped.
const VERIFY_STAMP_LINES: usize = 200;

/// The newest verification demand in `text` inside `horizon`.
///
/// Searched by bytes, not parsed as JSON, and that is forced by the file: the
/// IDE writes the server's stderr in chunks, with blank lines and its own
/// `[LS Main stderr]` prefix wherever a chunk starts, so a body that spans lines
/// does not survive as one JSON document. The one value needed is short and
/// never broken across lines, so it is read as a JSON string where it stands.
fn newest_verification(
    text: &str,
    now: crate::utils::LocalClock,
    horizon: Duration,
) -> Option<Verification> {
    let bytes = text.as_bytes();
    // ASCII lowercasing keeps every byte offset where it was.
    let lower = text.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let mut best: Option<Verification> = None;
    let mut from = 0usize;
    while let Some(off) = find(&lb[from..], b"validation") {
        let key = from + off;
        from = key + 1;
        let Some(value_at) = url_value_start(lb, key) else {
            continue;
        };
        let lo = key.saturating_sub(VERIFY_REACH);
        let hi = (key + VERIFY_REACH).min(bytes.len());
        if !has_reason(&bytes[lo..hi]) {
            continue;
        }
        let Some(url) = read_value(bytes, value_at).filter(|u| is_google_page(u)) else {
            continue;
        };
        let Some(ago) = stamp_before(text, key).and_then(|at| age_secs(at, now)) else {
            continue;
        };
        let ago = Duration::from_secs(ago as u64);
        if ago > horizon {
            continue;
        }
        // `<=`: of two in the same second, the later one in the file.
        if best.as_ref().is_none_or(|b| ago <= b.ago) {
            best = Some(Verification { ago, url });
        }
    }
    best
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Where the value of a link key starting at `key` begins, or `None` if the
/// word there is not one. Takes `validation_url` and `validationUrl` (and Go's
/// `ValidationURL`) - lowercased by the caller - but not
/// `validation_url_link_text`, which is the button's caption.
fn url_value_start(lower: &[u8], key: usize) -> Option<usize> {
    let rest = &lower[key..];
    let len = if rest.starts_with(b"validation_url") {
        14
    } else if rest.starts_with(b"validationurl") {
        13
    } else {
        return None;
    };
    if key > 0 && is_word(lower[key - 1]) {
        return None;
    }
    let mut i = key + len;
    if lower.get(i).is_some_and(|&c| is_word(c)) {
        return None;
    }
    // The key's own closing quote, escaped once when the body was logged as a
    // quoted string.
    if lower.get(i) == Some(&b'\\') {
        i += 1;
    }
    if lower.get(i) == Some(&b'"') {
        i += 1;
    }
    while lower.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
        i += 1;
    }
    if lower.get(i) != Some(&b':') {
        return None;
    }
    i += 1;
    while lower.get(i).is_some_and(|c| *c == b' ' || *c == b'\t') {
        i += 1;
    }
    Some(i)
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// `VALIDATION_REQUIRED` as a word of its own, so an enum listing it among
/// others still counts but a longer name that merely contains it does not.
fn has_reason(window: &[u8]) -> bool {
    let mut from = 0usize;
    while let Some(off) = find(&window[from..], VERIFY_REASON) {
        let at = from + off;
        let end = at + VERIFY_REASON.len();
        let before = at == 0 || !is_word(window[at - 1]);
        let after = window.get(end).is_none_or(|&c| !is_word(c));
        if before && after {
            return true;
        }
        from = at + 1;
    }
    false
}

/// The link at `at`, in whichever of the three shapes it was logged: a JSON
/// string (`"https:вЂ¦&вЂ¦"`), that string escaped once more because the whole
/// body was logged as a quoted string (`\"https:вЂ¦\\u0026вЂ¦\"`), or bare, as Go
/// prints a struct (`ValidationURL:https://вЂ¦ `). JSON escapes are decoded - a
/// link with `&` left in it opens a page with its parameters broken.
fn read_value(bytes: &[u8], at: usize) -> Option<String> {
    let rest = bytes.get(at..)?;
    if rest.starts_with(b"\\\"") {
        // Undo the outer quoting, then read what is left as the JSON string it was.
        let end = find(&rest[2..], b"\\\"")? + 2;
        let mut inner = Vec::with_capacity(end);
        let mut i = 2;
        while i < end {
            if rest[i] == b'\\' && i + 1 < end {
                inner.push(rest[i + 1]);
                i += 2;
            } else {
                inner.push(rest[i]);
                i += 1;
            }
        }
        let lit = format!("\"{}\"", String::from_utf8(inner).ok()?);
        return serde_json::from_str::<String>(&lit).ok();
    }
    if rest.first() == Some(&b'"') {
        let mut i = 1;
        while i < rest.len() {
            match rest[i] {
                b'\\' => i += 2,
                b'"' => {
                    let lit = std::str::from_utf8(&rest[..=i]).ok()?;
                    return serde_json::from_str::<String>(lit).ok();
                }
                b'\n' | b'\r' => return None,
                _ => i += 1,
            }
        }
        return None;
    }
    if rest.starts_with(b"https://") {
        let end = rest
            .iter()
            .position(|c| c.is_ascii_whitespace() || *c == b'}' || *c == b'"')
            .unwrap_or(rest.len());
        return String::from_utf8(rest[..end].to_vec()).ok();
    }
    None
}

/// Only a Google page over HTTPS is ever opened: the link comes out of a log
/// file, and a button that opens whatever a log line says is a button anyone
/// who can write a line there controls.
fn is_google_page(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    if url.len() > 8192 || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..host_end].to_ascii_lowercase();
    // No userinfo and no port: `https://google.com@evil/` names `evil`.
    if host.contains(['@', ':', '\\']) {
        return false;
    }
    host == "google.com" || host.ends_with(".google.com")
}

/// The stamp of the entry holding byte `pos`: the nearest stamped line at or
/// above it.
fn stamp_before(text: &str, pos: usize) -> Option<(u16, u16, u32)> {
    let line_end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    text[..line_end]
        .rsplit('\n')
        .take(VERIFY_STAMP_LINES)
        .find_map(parse_stamp)
}

/// Total size of the given logs. The cheap half of "is Antigravity doing
/// anything" - one `metadata` per file and no read at all - which is what the
/// window gates both its scans and its VPN measurement on. A log that has not
/// grown cannot have gained a refusal, and a client that is not writing is not
/// one whose route can have changed under us.
pub fn bytes_of(paths: &[PathBuf]) -> u64 {
    paths
        .iter()
        .filter_map(|p| fs::metadata(p).ok().map(|m| m.len()))
        .sum()
}

/// The last `max` bytes of a file, as text. The first line usually comes back
/// cut; that is the caller's problem and it handles it by refusing to place a
/// line it cannot parse.
fn tail(path: &Path, max: u64) -> Option<String> {
    let len = fs::metadata(path).ok()?.len();
    let start = len.saturating_sub(max);
    let mut f = File::open(path).ok()?;
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.take(len - start).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// glog's header вЂ” a level letter, `MMDD`, a space, then local `HH:MM:SS.ffffff`
/// вЂ” wherever in the line it sits.
///
/// Searched for rather than sliced off the front, and that is not defensive
/// programming: measured on both real logs, it is never at the front. The
/// Desktop server prefixes *every* line with `ERROR: logging before
/// google.Init: `, and the IDE wraps that same text again in its own
/// `2026-09-12 03:57:46.735 [error] [LS Main stderr] `. Anchoring at byte 0
/// found a stamp in neither file, so the window would have shown "С‚РёС…Рѕ" while
/// the user was staring at the error.
///
/// Returns `(month, day, second of day)`; there is no year in it, which is why
/// `age_secs` only ever places a line today or yesterday.
fn parse_stamp(line: &str) -> Option<(u16, u16, u32)> {
    let b = line.as_bytes();
    let limit = b.len().min(HEADER_SEARCH).saturating_sub(STAMP_LEN);
    (0..=limit).find_map(|i| stamp_at(line, i))
}

/// How far into a line the header is looked for. Both wrappers measured above
/// fit in well under a hundred bytes; the bound is what keeps a stamp-shaped
/// run of text deep inside a message from being read as one.
const HEADER_SEARCH: usize = 200;
/// `E0901 15:09:24` вЂ” the part that has to be there.
const STAMP_LEN: usize = 14;

/// The header if it starts exactly at `at`, and nothing otherwise. A slice that
/// lands mid-character answers `None` rather than panicking (`str::get`), which
/// is what makes scanning a line of arbitrary bytes safe.
fn stamp_at(line: &str, at: usize) -> Option<(u16, u16, u32)> {
    let b = line.as_bytes();
    if b.len() < at + STAMP_LEN {
        return None;
    }
    // glog's four levels. `is_ascii_alphabetic` would match the `E` of a word
    // followed by four digits, which is exactly the kind of thing a log message
    // contains.
    if !matches!(b[at], b'I' | b'W' | b'E' | b'F') {
        return None;
    }
    if b[at + 5] != b' ' || b[at + 8] != b':' || b[at + 11] != b':' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| line.get(r).and_then(|s| s.parse::<u32>().ok());
    let (month, day) = (num(at + 1..at + 3)?, num(at + 3..at + 5)?);
    let (h, m, s) = (
        num(at + 6..at + 8)?,
        num(at + 9..at + 11)?,
        num(at + 12..at + 14)?,
    );
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || h > 23 || m > 59 || s > 59 {
        return None;
    }
    Some((month as u16, day as u16, h * 3600 + m * 60 + s))
}

/// How long ago a stamp was, against the local clock now.
///
/// Only today and yesterday can be placed: the header carries no year, and the
/// window this is asked about is minutes. Anything else answers `None`, which
/// reads as "not recent" - the safe direction, since the cost of missing an old
/// refusal is nothing and the cost of inventing a fresh one is a false alarm.
fn age_secs(at: (u16, u16, u32), now: crate::utils::LocalClock) -> Option<u32> {
    let (month, day, sod) = at;
    if month == now.month && day == now.day {
        return if sod <= now.second_of_day {
            Some(now.second_of_day - sod)
        } else if sod - now.second_of_day <= CLOCK_SLACK {
            Some(0)
        } else {
            None
        };
    }
    if is_day_before((month, day), now) {
        return Some(now.second_of_day + 86_400 - sod);
    }
    None
}

fn is_day_before(at: (u16, u16), now: crate::utils::LocalClock) -> bool {
    if at.0 == now.month {
        return at.1 + 1 == now.day;
    }
    // The first of a month: yesterday is the last day of the one before, whose
    // number the header does not carry a year to settle. February is therefore
    // allowed both its lengths, and the only stamp that can be misplaced is a
    // 28 February read on 1 March of a leap year - in the first minutes after
    // midnight, once every four years, and the cost is one line of the window
    // saying "a few minutes ago" about a day-old refusal.
    now.day == 1 && at.0 == month_before(now.month) && is_last_day(at.0, at.1)
}

fn is_last_day(month: u16, day: u16) -> bool {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => day == 31,
        4 | 6 | 9 | 11 => day == 30,
        2 => day == 28 || day == 29,
        _ => false,
    }
}

fn month_before(m: u16) -> u16 {
    if m <= 1 {
        12
    } else {
        m - 1
    }
}


#[cfg(test)]
mod tests;

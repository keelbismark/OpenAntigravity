use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use rustls::pki_types::ServerName;
use rustls::ClientConnection;

use crate::dns_client;
use super::providers::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Outside every netblock the reference answered from - a proxy address.
    Substituted,
    /// A different address in the same netblock family. Google rotates its edge
    /// per query and per resolver, so this is almost always just another
    /// genuine Google address, not a substitution.
    Sibling,
    /// The same netblock the reference gave: a passthrough.
    Passthrough,
    /// No usable reference, or no addresses to compare.
    Unknown,
}

pub(super) fn block(addr: &IpAddr) -> (u8, u32) {
    match addr {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            (4, u32::from_be_bytes([o[0], o[1], 0, 0]))
        }
        IpAddr::V6(v6) => {
            let o = v6.octets();
            (6, u32::from_be_bytes([o[0], o[1], o[2], o[3]]))
        }
    }
}

pub fn classify(candidate: &[IpAddr], reference: &[IpAddr], proxy: &[IpAddr]) -> Verdict {
    if candidate.is_empty() {
        return Verdict::Unknown;
    }
    let reference: Vec<&IpAddr> = reference
        .iter()
        .filter(|a| match a {
            IpAddr::V4(v4) => !REFERENCE_STUBS.contains(v4),
            IpAddr::V6(_) => true,
        })
        .collect();
    if reference.is_empty() {
        return Verdict::Unknown;
    }
    let families: Vec<u8> = candidate.iter().map(|a| block(a).0).collect();
    let comparable: Vec<&&IpAddr> = reference
        .iter()
        .filter(|a| families.contains(&block(a).0))
        .collect();
    if comparable.is_empty() {
        return Verdict::Unknown;
    }

    let ref_blocks: Vec<(u8, u32)> = comparable.iter().map(|a| block(a)).collect();
    if candidate.iter().any(|a| ref_blocks.contains(&block(a))) {
        return Verdict::Passthrough;
    }
    if proxy.is_empty() {
        return Verdict::Substituted;
    }
    let proxy_blocks: Vec<(u8, u32)> = proxy.iter().map(block).collect();
    if candidate.iter().any(|a| proxy_blocks.contains(&block(a))) {
        return Verdict::Substituted;
    }
    Verdict::Sibling
}

pub(super) fn rank(v: Verdict) -> u8 {
    match v {
        Verdict::Substituted => 3,
        Verdict::Unknown => 2,
        Verdict::Sibling => 1,
        Verdict::Passthrough => 0,
    }
}

const LIVENESS_PORT: u16 = 443;
const LIVENESS_BUDGET: Duration = Duration::from_millis(500);
const LIVENESS_BUDGET_WARM: Duration = Duration::from_millis(2500);
pub(super) const LIVENESS_TTL_ALIVE: Duration = Duration::from_secs(20);
const LIVENESS_TTL_DEAD: Duration = Duration::from_secs(60);

static LIVENESS: Mutex<Option<HashMap<IpAddr, (bool, Instant)>>> = Mutex::new(None);

pub(super) fn cached_liveness(addr: &IpAddr) -> Option<bool> {
    let guard = LIVENESS.lock().ok()?;
    let (alive, at) = guard.as_ref()?.get(addr)?;
    let ttl = if *alive {
        LIVENESS_TTL_ALIVE
    } else {
        LIVENESS_TTL_DEAD
    };
    (at.elapsed() < ttl).then_some(*alive)
}

pub(super) fn remember_liveness(addr: IpAddr, alive: bool) {
    if let Ok(mut guard) = LIVENESS.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(addr, (alive, Instant::now()));
    }
}

pub(super) fn reachable(addr: IpAddr, budget: Duration, sni: Option<&str>) -> bool {
    let Ok(sock) =
        std::net::TcpStream::connect_timeout(&SocketAddr::new(addr, LIVENESS_PORT), budget)
    else {
        return false;
    };
    let Some(name) = sni else {
        return true;
    };
    let Ok(server) = ServerName::try_from(name.to_string()) else {
        return false;
    };
    let Ok(mut conn) = ClientConnection::new(crate::proxy::probe_config(), server) else {
        return false;
    };
    sock.set_read_timeout(Some(budget)).ok();
    sock.set_write_timeout(Some(budget)).ok();
    let mut sock = sock;
    let deadline = Instant::now() + budget;
    while conn.is_handshaking() {
        if Instant::now() >= deadline {
            return false;
        }
        if conn.wants_write() && conn.write_tls(&mut sock).is_err() {
            return false;
        }
        if conn.wants_read() {
            match conn.read_tls(&mut sock) {
                Ok(0) => return false,
                Ok(_) => {
                    if conn.process_new_packets().is_err() {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        }
    }
    true
}

pub(super) fn dead_addrs(addrs: &[IpAddr], fresh: bool, sni: Option<&str>) -> Vec<IpAddr> {
    let mut dead = Vec::new();
    let mut unknown = Vec::new();
    for a in addrs {
        match if fresh { None } else { cached_liveness(a) } {
            Some(true) => {}
            Some(false) => dead.push(*a),
            None => unknown.push(*a),
        }
    }
    if unknown.is_empty() {
        return dead;
    }

    let budget = if fresh {
        LIVENESS_BUDGET_WARM
    } else {
        LIVENESS_BUDGET
    };

    let (tx, rx) = mpsc::channel::<(IpAddr, bool)>();
    for a in &unknown {
        let tx = tx.clone();
        let addr = *a;
        let sni = if crate::settings::verify_tls_enabled() {
            sni.map(|s| s.to_string())
        } else {
            None
        };
        thread::spawn(move || {
            let ok = reachable(addr, budget, sni.as_deref());
            tx.send((addr, ok)).ok();
        });
    }
    drop(tx);

    let deadline = Instant::now() + budget;
    let mut answered: Vec<IpAddr> = Vec::new();
    while answered.len() < unknown.len() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match rx.recv_timeout(left) {
            Ok((addr, alive)) => {
                answered.push(addr);
                remember_liveness(addr, alive);
                if !alive {
                    dead.push(addr);
                }
            }
            Err(_) => break,
        }
    }
    for a in unknown {
        if !answered.contains(&a) {
            remember_liveness(a, false);
            dead.push(a);
        }
    }
    dead
}

pub(super) fn drop_dead_addrs(reply: Vec<u8>, fresh: bool, sni: Option<&str>) -> Vec<u8> {
    let addrs = dns_client::answer_addrs(&reply);
    if addrs.len() < 2 {
        return reply;
    }
    let dead = dead_addrs(&addrs, fresh, sni);
    if dead.is_empty() {
        return reply;
    }
    dns_client::without_addrs(&reply, &dead).unwrap_or(reply)
}

const PROXY_SET_TTL: Duration = Duration::from_secs(30 * 60);

type ProxySetMap = HashMap<usize, (Vec<IpAddr>, Instant)>;
static PROXY_SET: Mutex<Option<ProxySetMap>> = Mutex::new(None);

pub(super) fn known_proxy_addrs(idx: usize) -> Vec<IpAddr> {
    let Ok(guard) = PROXY_SET.lock() else {
        return Vec::new();
    };
    guard
        .as_ref()
        .and_then(|m| m.get(&idx))
        .filter(|(_, at)| at.elapsed() < PROXY_SET_TTL)
        .map(|(addrs, _)| addrs.clone())
        .unwrap_or_default()
}

pub(super) fn proxy_addrs_are_stale(idx: usize) -> bool {
    let Ok(guard) = PROXY_SET.lock() else {
        return false;
    };
    guard
        .as_ref()
        .and_then(|m| m.get(&idx))
        .is_none_or(|(_, at)| at.elapsed() >= PROXY_SET_TTL)
}

pub(super) fn learn_proxy_addrs(idx: usize, addrs: Vec<IpAddr>) {
    if addrs.is_empty() {
        return;
    }
    if let Ok(mut guard) = PROXY_SET.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        let entry = map
            .entry(idx)
            .or_insert_with(|| (Vec::new(), Instant::now()));
        for a in addrs {
            if !entry.0.contains(&a) {
                entry.0.push(a);
            }
        }
        entry.1 = Instant::now();
    }
}

pub(super) fn ensure_proxy_sets_learned(if_index: u32) {
    if !(0..PROVIDERS.len()).any(proxy_addrs_are_stale) {
        return;
    }
    const ROUNDS: usize = 2;
    let mut done = 0;
    for &picked in CONTROL_NAMES {
        if done >= ROUNDS {
            break;
        }
        let control = dns_client::build_query(picked, 0x0C71);
        let mut cref: Vec<IpAddr> = Vec::new();
        for server in REFERENCE_V4 {
            if let Ok(ip) = server.parse::<Ipv4Addr>() {
                if let Ok(reply) = dns_client::query_raw_via(&control, ip, if_index, QUERY_TIMEOUT)
                {
                    cref.extend(dns_client::answer_addrs(&reply));
                }
            }
        }
        let usable = cref.iter().any(|a| match a {
            IpAddr::V4(v) => !REFERENCE_STUBS.contains(v),
            IpAddr::V6(_) => true,
        });
        if !usable {
            continue;
        }
        for (idx, provider) in PROVIDERS.iter().enumerate() {
            if let Some(reply) = ask_provider(provider, &control, if_index, QUERY_TIMEOUT) {
                let addrs = dns_client::answer_addrs(&reply);
                if classify(&addrs, &cref, &[]) == Verdict::Substituted {
                    learn_proxy_addrs(idx, addrs);
                }
            }
        }
        done += 1;
    }
}

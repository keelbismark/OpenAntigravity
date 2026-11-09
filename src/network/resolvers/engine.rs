use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use crate::dns_client;
use super::providers::*;
use super::verdict::*;

pub(super) struct Answer {
    pub(super) reply: Vec<u8>,
    pub(super) provider: &'static str,
    pub(super) verdict: Verdict,
    pub(super) at: Instant,
    pub(super) good_for: Duration,
    pub(super) wanted: Instant,
}

pub(super) static ANSWERS: Mutex<Option<HashMap<Vec<u8>, Answer>>> = Mutex::new(None);

pub(super) static ROTATION: AtomicUsize = AtomicUsize::new(0);

pub(super) type ChoiceMap = HashMap<(String, u16), (usize, Verdict, Instant)>;
pub(super) static CHOICE: Mutex<Option<ChoiceMap>> = Mutex::new(None);

pub(super) fn cached_choice(key: &(String, u16)) -> Option<(usize, Verdict)> {
    let guard = CHOICE.lock().ok()?;
    let map = guard.as_ref()?;
    let (idx, verdict, at) = map.get(key)?;
    (at.elapsed() < CHOICE_TTL).then_some((*idx, *verdict))
}

pub(super) const MAX_CHOICE_ENTRIES: usize = 256;

pub(super) fn remember_choice(key: (String, u16), idx: usize, verdict: Verdict) {
    if let Ok(mut guard) = CHOICE.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        // Prune expired entries to prevent unbounded memory growth (Q.E.D. Invariant 6)
        map.retain(|_, (_, _, at)| at.elapsed() < CHOICE_TTL);
        if map.len() >= MAX_CHOICE_ENTRIES {
            if let Some(oldest_key) = map.iter().min_by_key(|(_, (_, _, at))| *at).map(|(k, _)| k.clone()) {
                map.remove(&oldest_key);
            }
        }
        map.insert(key, (idx, verdict, Instant::now()));
    }
}

pub(super) fn forget_choice(key: &(String, u16)) {
    if let Ok(mut guard) = CHOICE.lock() {
        if let Some(map) = guard.as_mut() {
            map.remove(key);
        }
    }
}

pub(super) fn answer_key(query: &[u8]) -> Option<Vec<u8>> {
    if query.len() < 12 {
        return None;
    }
    let mut key = query.to_vec();
    key[0] = 0;
    key[1] = 0;
    Some(key)
}

pub(super) fn cached_answer(
    query: &[u8],
    stale: bool,
    from_client: bool,
) -> Option<(Vec<u8>, &'static str, Verdict)> {
    let key = answer_key(query)?;
    let mut guard = ANSWERS.lock().ok()?;
    let hit = guard.as_mut()?.get_mut(&key)?;
    let age = hit.at.elapsed();
    let usable = if stale {
        hit.verdict == Verdict::Substituted && age < ANSWER_STALE_TTL
    } else {
        age < hit.good_for
    };
    if !usable {
        return None;
    }
    if from_client {
        hit.wanted = Instant::now();
    }
    let mut reply = dns_client::age_reply(&hit.reply, age.as_secs() as u32)?;
    reply[0..2].copy_from_slice(&query[0..2]);
    Some((reply, hit.provider, hit.verdict))
}

pub(super) fn remember_answer(
    query: &[u8],
    reply: &[u8],
    provider: &'static str,
    verdict: Verdict,
    from_client: bool,
) {
    let (Some(key), Some(ttl)) = (answer_key(query), dns_client::answer_ttl(reply)) else {
        return;
    };
    let Ok(mut guard) = ANSWERS.lock() else {
        return;
    };
    let map = guard.get_or_insert_with(HashMap::new);
    if verdict != Verdict::Substituted {
        if let Some(prev) = map.get(&key) {
            if prev.verdict == Verdict::Substituted && prev.at.elapsed() < ANSWER_STALE_TTL {
                return;
            }
        }
    }
    let now = Instant::now();
    let wanted = match map.get(&key) {
        Some(prev) if !from_client => prev.wanted,
        _ => now,
    };
    // Bound maximum cache capacity (Q.E.D. Invariant 6)
    const MAX_ANSWER_ENTRIES: usize = 512;
    if map.len() >= MAX_ANSWER_ENTRIES {
        if let Some(oldest_key) = map.iter().min_by_key(|(_, a)| a.wanted).map(|(k, _)| k.clone()) {
            map.remove(&oldest_key);
        }
    }
    map.insert(
        key,
        Answer {
            reply: reply.to_vec(),
            provider,
            verdict,
            at: now,
            good_for: ANSWER_TTL.min(Duration::from_secs(ttl.into())),
            wanted,
        },
    );
    map.retain(|_, a| a.wanted.elapsed() < ANSWER_IDLE);
}

pub(super) fn warm_queries(names: &[&str]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut covered: Vec<String> = Vec::new();
    if let Ok(guard) = ANSWERS.lock() {
        if let Some(map) = guard.as_ref() {
            for key in map.keys() {
                let Some(name) = dns_client::question_name(key) else {
                    continue;
                };
                if names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                    covered.push(name);
                    out.push(key.clone());
                }
            }
        }
    }
    for n in names {
        if !covered.iter().any(|c| c.eq_ignore_ascii_case(n)) {
            out.push(dns_client::build_query(n, 0));
        }
    }
    out
}

pub fn warm(names: &[&str], if_index: u32) {
    for mut query in warm_queries(names) {
        let id = ROTATION.fetch_add(1, Ordering::Relaxed) as u16;
        query[0..2].copy_from_slice(&id.to_be_bytes());
        refresh(&query, if_index);
    }
}

enum Heat {
    Provider(usize, Vec<u8>),
    Reference(Vec<IpAddr>),
    Control(usize, Vec<IpAddr>),
    ControlReference(Vec<IpAddr>),
}

static VPN_ACTIVE: AtomicBool = AtomicBool::new(false);
static FORCE_SUBSTITUTE_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

pub fn set_vpn_active(active: bool) {
    VPN_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn vpn_is_active() -> bool {
    VPN_ACTIVE.load(Ordering::Relaxed) && !substitution_forced()
}

pub fn tunnel_carries_client() -> bool {
    VPN_ACTIVE.load(Ordering::Relaxed)
}

pub fn force_substitution(for_how_long: Duration) {
    if let Ok(mut until) = FORCE_SUBSTITUTE_UNTIL.lock() {
        *until = Some(Instant::now() + for_how_long);
    }
}

pub fn substitution_forced() -> bool {
    substitution_forced_for().is_some()
}

pub fn substitution_forced_for() -> Option<Duration> {
    FORCE_SUBSTITUTE_UNTIL
        .lock()
        .ok()
        .and_then(|u| *u)
        .and_then(|until| until.checked_duration_since(Instant::now()))
}

static SERVED: Mutex<Option<HashMap<String, (Verdict, Instant)>>> = Mutex::new(None);
const SERVED_TTL: Duration = Duration::from_secs(10 * 60);

pub(super) fn note_served(name: &str, verdict: Verdict) {
    if let Ok(mut guard) = SERVED.lock() {
        guard.get_or_insert_with(HashMap::new).insert(
            name.trim_end_matches('.').to_ascii_lowercase(),
            (verdict, Instant::now()),
        );
    }
}

pub fn served_verdict(name: &str) -> Option<Verdict> {
    let guard = SERVED.lock().ok()?;
    let (verdict, at) = guard
        .as_ref()?
        .get(&name.trim_end_matches('.').to_ascii_lowercase())?;
    (at.elapsed() < SERVED_TTL).then_some(*verdict)
}

pub fn forget_names(names: &[&str]) {
    let wanted: Vec<String> = names
        .iter()
        .map(|n| n.trim_end_matches('.').to_ascii_lowercase())
        .collect();
    if let Ok(mut guard) = CHOICE.lock() {
        if let Some(map) = guard.as_mut() {
            map.retain(|(name, _), _| {
                !wanted.contains(&name.trim_end_matches('.').to_ascii_lowercase())
            });
        }
    }
    if let Ok(mut guard) = ANSWERS.lock() {
        if let Some(map) = guard.as_mut() {
            for (key, answer) in map.iter_mut() {
                let hit = dns_client::question_name(key)
                    .map(|q| q.trim_end_matches('.').to_ascii_lowercase())
                    .is_some_and(|q| wanted.contains(&q));
                if hit {
                    answer.good_for = Duration::ZERO;
                    answer.verdict = Verdict::Unknown;
                }
            }
        }
    }
}

fn resolve_through_tunnel(
    query: &[u8],
    from_client: bool,
) -> Option<(Vec<u8>, &'static str, Verdict)> {
    for server in REFERENCE_V4 {
        let Ok(ip) = server.parse::<Ipv4Addr>() else {
            continue;
        };
        let budget = if from_client {
            FAST_PATH_TIMEOUT
        } else {
            QUERY_TIMEOUT
        };
        if let Ok(reply) = dns_client::query_raw_via(query, ip, 0, budget) {
            let reply = dns_client::cap_ttl(&reply, PASSTHROUGH_TTL).unwrap_or(reply);
            remember_answer(query, &reply, "VPN", Verdict::Passthrough, from_client);
            return Some((reply, "VPN", Verdict::Passthrough));
        }
    }
    None
}

pub fn resolve_best(query: &[u8], if_index: u32) -> Option<(Vec<u8>, &'static str, Verdict)> {
    let hit = cached_answer(query, false, true).or_else(|| resolve_upstream(query, if_index, true));
    note_served_a(query, &hit);
    hit
}

pub(super) fn note_served_a(query: &[u8], hit: &Option<(Vec<u8>, &'static str, Verdict)>) {
    if dns_client::question_type(query) != Some(QTYPE_A) {
        return;
    }
    if let (Some(name), Some((_, _, verdict))) = (dns_client::question_name(query), hit) {
        note_served(&name, *verdict);
    }
}

pub fn refresh(query: &[u8], if_index: u32) -> Option<(Vec<u8>, &'static str, Verdict)> {
    let hit = resolve_upstream(query, if_index, false);
    note_served_a(query, &hit);
    hit
}

fn resolve_upstream(
    query: &[u8],
    if_index: u32,
    from_client: bool,
) -> Option<(Vec<u8>, &'static str, Verdict)> {
    let qtype = dns_client::question_type(query).unwrap_or(0);
    let name = dns_client::question_name(query).unwrap_or_default();
    let key = (name, qtype);

    if vpn_is_active() {
        if let Some(hit) = resolve_through_tunnel(query, from_client) {
            return Some(hit);
        }
    }

    let classifiable = qtype == QTYPE_A || qtype == QTYPE_AAAA;
    let one_shot = if from_client {
        FAST_PATH_TIMEOUT
    } else {
        QUERY_TIMEOUT
    };

    if let Some((idx, verdict)) = cached_choice(&key) {
        let budget = one_shot_floor(&PROVIDERS[idx], one_shot);
        if let Some(reply) = ask_provider(&PROVIDERS[idx], query, if_index, budget) {
            let reply = vet(reply, verdict, !from_client, Some(key.0.as_str()));
            remember_answer(query, &reply, PROVIDERS[idx].name, verdict, from_client);
            return prefer_substituted(query, from_client, (reply, PROVIDERS[idx].name, verdict));
        }
        forget_choice(&key);
    }

    if !classifiable {
        let start = ROTATION.fetch_add(1, Ordering::Relaxed);
        for step in 0..PROVIDERS.len() {
            let idx = (start + step) % PROVIDERS.len();
            if let Some(reply) = ask_provider(&PROVIDERS[idx], query, if_index, one_shot) {
                return Some((reply, PROVIDERS[idx].name, Verdict::Unknown));
            }
        }
        return None;
    }

    let (tx, rx) = mpsc::channel::<Heat>();
    for (idx, provider) in PROVIDERS.iter().enumerate() {
        let tx = tx.clone();
        let q = query.to_vec();
        thread::spawn(move || {
            if let Some(reply) = ask_provider(provider, &q, if_index, QUERY_TIMEOUT) {
                tx.send(Heat::Provider(idx, reply)).ok();
            }
        });
    }
    for server in REFERENCE_V4 {
        let Ok(ip) = server.parse::<Ipv4Addr>() else {
            continue;
        };
        let tx = tx.clone();
        let q = query.to_vec();
        thread::spawn(move || {
            if let Ok(reply) = dns_client::query_raw_via(&q, ip, if_index, QUERY_TIMEOUT) {
                tx.send(Heat::Reference(dns_client::answer_addrs(&reply)))
                    .ok();
            }
        });
    }

    let learning = (0..PROVIDERS.len()).any(proxy_addrs_are_stale);
    if learning {
        let picked = CONTROL_NAMES[ROTATION.fetch_add(1, Ordering::Relaxed) % CONTROL_NAMES.len()];
        let control = dns_client::build_query(picked, 0x0C71);
        for (idx, provider) in PROVIDERS.iter().enumerate() {
            let tx = tx.clone();
            let q = control.clone();
            thread::spawn(move || {
                if let Some(reply) = ask_provider(provider, &q, if_index, QUERY_TIMEOUT) {
                    tx.send(Heat::Control(idx, dns_client::answer_addrs(&reply)))
                        .ok();
                }
            });
        }
        if let Ok(ip) = REFERENCE_V4[0].parse::<Ipv4Addr>() {
            let tx = tx.clone();
            thread::spawn(move || {
                if let Ok(reply) = dns_client::query_raw_via(&control, ip, if_index, QUERY_TIMEOUT)
                {
                    tx.send(Heat::ControlReference(dns_client::answer_addrs(&reply)))
                        .ok();
                }
            });
        }
    }
    drop(tx);

    let deadline = Instant::now() + RACE_BUDGET;
    let preferred = preferred_provider(&key.0);
    let grace_until = Instant::now() + PREFERRED_GRACE;
    let mut reference: Vec<IpAddr> = Vec::new();
    let mut replies: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut control: Vec<(usize, Vec<IpAddr>)> = Vec::new();
    let mut control_reference: Vec<IpAddr> = Vec::new();
    let mut got_reference = false;
    let mut got_control_reference = false;

    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match rx.recv_timeout(left) {
            Ok(Heat::Reference(addrs)) => {
                reference.extend(addrs);
                got_reference = true;
            }
            Ok(Heat::Provider(idx, reply)) => replies.push((idx, reply)),
            Ok(Heat::Control(idx, addrs)) => control.push((idx, addrs)),
            Ok(Heat::ControlReference(addrs)) => {
                control_reference = addrs;
                got_control_reference = true;
            }
            Err(_) => break,
        }

        if got_control_reference {
            while let Some((idx, addrs)) = control.pop() {
                if classify(&addrs, &control_reference, &[]) == Verdict::Substituted {
                    learn_proxy_addrs(idx, addrs);
                }
            }
        }

        let still_learning = learning && !got_control_reference;
        let holding_for_preferred = preferred
            .is_some_and(|p| !replies.iter().any(|(i, _)| *i == p) && Instant::now() < grace_until);
        if got_reference && !replies.is_empty() && !still_learning && !holding_for_preferred {
            let hit = preferred
                .and_then(|p| replies.iter().position(|(i, _)| *i == p))
                .filter(|&h| {
                    classify(
                        &dns_client::answer_addrs(&replies[h].1),
                        &reference,
                        &known_proxy_addrs(replies[h].0),
                    ) == Verdict::Substituted
                })
                .or_else(|| pick(&replies, &reference, Verdict::Substituted));
            if let Some(hit) = hit {
                let name = key.0.clone();
                remember_choice(key, replies[hit].0, Verdict::Substituted);
                let (idx, reply) = replies.swap_remove(hit);
                let reply = vet(reply, Verdict::Substituted, !from_client, Some(&name));
                remember_answer(
                    query,
                    &reply,
                    PROVIDERS[idx].name,
                    Verdict::Substituted,
                    from_client,
                );
                return Some((reply, PROVIDERS[idx].name, Verdict::Substituted));
            }
        }
        if replies.len() == PROVIDERS.len() && got_reference && !still_learning {
            break;
        }
    }

    if replies.is_empty() {
        return cached_answer(query, true, from_client);
    }
    let best = best_of(&replies, &reference);
    if from_client && !replies.is_empty() && !dns_client::answer_addrs(&replies[best].1).is_empty()
    {
        let heard: Vec<&str> = replies.iter().map(|(i, _)| PROVIDERS[*i].name).collect();
        crate::dns_forwarder::log_race(&key.0, &heard, got_reference);
    }
    let verdict = classify(
        &dns_client::answer_addrs(&replies[best].1),
        &reference,
        &known_proxy_addrs(replies[best].0),
    );
    forget_choice(&key);
    let (idx, reply) = replies.swap_remove(best);
    let reply = vet(reply, verdict, !from_client, Some(key.0.as_str()));
    remember_answer(query, &reply, PROVIDERS[idx].name, verdict, from_client);
    prefer_substituted(query, from_client, (reply, PROVIDERS[idx].name, verdict))
}

pub(super) fn vet(reply: Vec<u8>, verdict: Verdict, fresh: bool, sni: Option<&str>) -> Vec<u8> {
    if verdict == Verdict::Substituted {
        drop_dead_addrs(reply, fresh, sni)
    } else {
        dns_client::cap_ttl(&reply, PASSTHROUGH_TTL).unwrap_or(reply)
    }
}

pub(super) fn prefer_substituted(
    query: &[u8],
    from_client: bool,
    fresh: (Vec<u8>, &'static str, Verdict),
) -> Option<(Vec<u8>, &'static str, Verdict)> {
    if fresh.2 == Verdict::Substituted {
        return Some(fresh);
    }
    cached_answer(query, true, from_client).or(Some(fresh))
}

pub fn substituting_addrs(name: &str, if_index: u32) -> Vec<&'static str> {
    ensure_proxy_sets_learned(if_index);
    let query = dns_client::build_query(name, 0x6767);

    let mut reference: Vec<IpAddr> = Vec::new();
    for server in REFERENCE_V4 {
        if let Ok(ip) = server.parse::<Ipv4Addr>() {
            if let Ok(reply) = dns_client::query_raw_via(&query, ip, if_index, QUERY_TIMEOUT) {
                reference.extend(dns_client::answer_addrs(&reply));
            }
        }
    }

    let mut out = Vec::new();
    for (idx, provider) in PROVIDERS.iter().enumerate() {
        let Some(reply) = ask_provider(provider, &query, if_index, QUERY_TIMEOUT) else {
            continue;
        };
        let addrs = dns_client::answer_addrs(&reply);
        let known = known_proxy_addrs(idx);
        if !known.is_empty() && classify(&addrs, &reference, &known) == Verdict::Substituted {
            if let Some(first) = provider.v4.first() {
                out.push(*first);
            }
        }
    }
    out
}

pub fn resolve_a_best(name: &str, if_index: u32) -> Option<(Vec<Ipv4Addr>, &'static str, Verdict)> {
    let query = dns_client::build_query(name, 0x5A5A);
    let (reply, provider, verdict) = resolve_best(&query, if_index)?;
    let addrs: Vec<Ipv4Addr> = dns_client::answer_addrs(&reply)
        .into_iter()
        .filter_map(|a| match a {
            IpAddr::V4(v4) => Some(v4),
            IpAddr::V6(_) => None,
        })
        .collect();
    if addrs.is_empty() {
        return None;
    }
    Some((addrs, provider, verdict))
}

pub fn genuine_a(name: &str) -> Vec<Ipv4Addr> {
    const TTL: Duration = Duration::from_secs(60);
    const BUDGET: Duration = Duration::from_secs(2);
    type AddrCacheMap = HashMap<String, (Vec<Ipv4Addr>, Instant)>;
    static CACHE: Mutex<Option<AddrCacheMap>> = Mutex::new(None);

    let key = name.trim_end_matches('.').to_ascii_lowercase();
    if let Ok(guard) = CACHE.lock() {
        if let Some((addrs, at)) = guard.as_ref().and_then(|m| m.get(&key)) {
            if at.elapsed() < TTL && !addrs.is_empty() {
                return addrs.clone();
            }
        }
    }
    let query = dns_client::build_query(&key, 0x7A7A);
    let mut out: Vec<Ipv4Addr> = Vec::new();
    for server in REFERENCE_V4 {
        let Ok(ip) = server.parse::<Ipv4Addr>() else {
            continue;
        };
        if let Ok(reply) = dns_client::query_raw_via(&query, ip, 0, BUDGET) {
            out = dns_client::answer_addrs(&reply)
                .into_iter()
                .filter_map(|a| match a {
                    IpAddr::V4(v4) if is_public_v4(v4) => Some(v4),
                    _ => None,
                })
                .collect();
            if !out.is_empty() {
                break;
            }
        }
    }
    if !out.is_empty() {
        if let Ok(mut guard) = CACHE.lock() {
            guard
                .get_or_insert_with(HashMap::new)
                .insert(key, (out.clone(), Instant::now()));
        }
    }
    out
}

fn is_public_v4(v4: Ipv4Addr) -> bool {
    let o = v4.octets();
    !(REFERENCE_STUBS.contains(&v4)
        || v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || o[0] == 0
        || (o[0] == 100 && (64..128).contains(&o[1]))
        || (o[0] == 198 && (18..20).contains(&o[1]))
        || o[0] >= 224)
}

fn pick(replies: &[(usize, Vec<u8>)], reference: &[IpAddr], want: Verdict) -> Option<usize> {
    let start = ROTATION.load(Ordering::Relaxed);
    let mut best: Option<usize> = None;
    for i in 0..PROVIDERS.len() {
        let wanted_provider = (start + i) % PROVIDERS.len();
        let Some(pos) = replies.iter().position(|(idx, _)| *idx == wanted_provider) else {
            continue;
        };
        let verdict = classify(
            &dns_client::answer_addrs(&replies[pos].1),
            reference,
            &known_proxy_addrs(wanted_provider),
        );
        if verdict == want {
            best = Some(pos);
            break;
        }
    }
    if best.is_some() {
        ROTATION.fetch_add(1, Ordering::Relaxed);
    }
    best
}

fn best_of(replies: &[(usize, Vec<u8>)], reference: &[IpAddr]) -> usize {
    let start = ROTATION.fetch_add(1, Ordering::Relaxed);
    let mut best = 0usize;
    let mut best_rank = 0u8;
    let mut seen = false;
    for i in 0..PROVIDERS.len() {
        let wanted_provider = (start + i) % PROVIDERS.len();
        let Some(pos) = replies.iter().position(|(idx, _)| *idx == wanted_provider) else {
            continue;
        };
        let r = rank(classify(
            &dns_client::answer_addrs(&replies[pos].1),
            reference,
            &known_proxy_addrs(wanted_provider),
        ));
        if !seen || r > best_rank {
            seen = true;
            best_rank = r;
            best = pos;
        }
    }
    best
}

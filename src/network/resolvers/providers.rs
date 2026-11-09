use std::net::Ipv4Addr;
use std::time::Duration;

use crate::dns_client;

// Choosing an unblock resolver at query time instead of trusting one forever.
//
// Why this module exists: on 2026-08-24 xbox-dns.ru silently dropped
// `cloudcode-pa.googleapis.com` from the list of names it substitutes. Measured
// over a raw socket, it answers that name with the genuine Google address
// (172.217.x, TTL 300) while still substituting `generativelanguage` to its
// proxy (87.228.47.204, TTL 60). comss.one and geohide.ru had done the same.
// Nothing in the tool noticed: the relay forwarded, the NRPT rules applied, the
// answer arrived - it was simply not a substituted answer any more.
//
// A single hardcoded resolver cannot detect that, so the relay now asks every
// provider at once and, in parallel, a reference resolver that is known NOT to
// substitute anything. An answer equal to the reference is a passthrough, and
// the next provider is tried; an answer that differs is a real substitution.
// When a provider re-adds a name, or a new provider is added below, it starts
// being used with no code change and no release.

/// How a provider is reached.
///
/// The distinction is not cosmetic: `v4`/`v6` are also what an NRPT rule lists
/// as fallback nameservers (I5, I22), and Windows can only be pointed at a
/// plain resolver. A DoH endpoint answers no UDP query at all, so putting one
/// in a rule would hand a name a nameserver that never replies. That is
/// enforced structurally - a `Doh` provider carries **empty** `v4`/`v6`, so
/// `fallback_v4`, `all_v6` and `substituting_addrs` all skip it without needing
/// to know about transports - and asserted by `a_doh_provider_is_never_offered_
/// as_a_nameserver`.
pub enum Transport {
    /// Plain DNS on UDP:53, queried over a socket pinned to the ISP interface.
    Udp,
    /// DNS-over-HTTPS. Reachable only through the relay, never by Windows.
    Doh(&'static crate::doh::Endpoint),
}

/// One unblock service. Addresses are tried in order; the first that answers
/// speaks for the provider, so a dead front-end does not cost it the race.
pub struct Provider {
    pub name: &'static str,
    pub v4: &'static [&'static str],
    pub v6: &'static [&'static str],
    pub transport: Transport,
}

/// Services that substitute Google/AI endpoints for clients they geolocate to a
/// blocked region. All three were verified to answer over plain UDP:53 and to
/// front a real SNI proxy (see kb/dns.md); their lists differ and change
/// without notice, which is exactly why the choice is made per query.
/// The provider names, in pool order, for the window's per-provider switches.
///
/// Names only. The addresses stay where they are: a switch list is a new place
/// for them to be read out of, and what a user needs in order to turn one off is
/// the name they already see in the log line.
pub fn provider_names() -> Vec<&'static str> {
    PROVIDERS.iter().map(|p| p.name).collect()
}

pub const PROVIDERS: &[Provider] = &[
    // First on purpose. Measured 2026-08-30 against 8.8.8.8: it substitutes
    // `cloudcode-pa.googleapis.com` for one of its own egress addresses (TTL 60),
    // and that address accepts the SNI and answers as Google's own frontend
    // (`Server: ESF`, valid certificate, TLS handshake 0.30 s).
    //
    // The address that measurement named was `186.246.45.126`, and it is written
    // out here no longer ON PURPOSE: the service deleted that egress and returned
    // the address to its hoster, and something else answers on it today (checked
    // 2026-09-12, G43). The substitution still works — the service just answers
    // with whichever egress it currently runs — so the FINDING holds and only the
    // number expired. Do not re-derive it from a stranger's host.
    // `cloudcode-pa` is the name a 22-resolver
    // sweep found **nobody** substituting (S9) - the whole reason the client used
    // to be pushed onto `daily-` instead (N15). It substitutes `daily-` and
    // `generativelanguage` from the same address, and correctly passes
    // `jetski-webchannel` through, so it is not a blanket rewriter.
    //
    // `v4`/`v6` are empty because it speaks **only** DoH over HTTP/2 - HTTP/1.1
    // gets a 505 and its two addresses answer nothing on UDP:53. See `Transport`
    // for why that emptiness is load-bearing rather than an omission.
    Provider {
        name: "dns-ai.ru",
        v4: &[],
        v6: &[],
        transport: Transport::Doh(&DNS_AI),
    },
    Provider {
        name: "comss.one",
        v4: &["83.220.169.155", "212.109.195.93", "195.133.25.16"],
        v6: &[],
        transport: Transport::Udp,
    },
    Provider {
        name: "geohide.ru",
        v4: &["45.155.204.190", "37.230.192.51"],
        v6: &["2a0c:9300:0:54::1"],
        transport: Transport::Udp,
    },
    // xbox-dns.ru is not here any more (owner, 2.14.0_1). Measured over the ISP
    // link on 2026-09-18, both its resolvers answer both gate hosts with genuine
    // Google (`172.217.x`, the set 8.8.8.8 gives) and substitute only
    // `generativelanguage`, which is not ours to route (D18). It never won a race
    // for either name, so all it contributed was a third party seeing the query,
    // and a row in the window promising something it does not do. Its patcher
    // reaches the gate hosts through its credentialed CONNECT relay instead, and
    // that path is already a route of its own (`routes::Kind::Relay`).
];

/// Addresses are the three `dns.dns-ai.ru` resolves to, hardcoded so the relay can
/// reach it before anything else resolves. Safe because the certificate still has
/// to prove the name (`*.dns-ai.ru`, verified live), so a stale or poisoned
/// address fails the handshake instead of becoming a silent man-in-the-middle.
pub static DNS_AI: crate::doh::Endpoint = crate::doh::Endpoint {
    host: "dns.dns-ai.ru",
    path: "/dns-query",
    addrs: &["192.144.59.14", "186.246.49.127", "94.232.43.149"],
};

/// Resolvers used only to recognise an unsubstituted answer. They must be
/// services with no geo-unblocking of their own - that is the whole point of
/// the comparison - and are never used to answer the client.
pub const REFERENCE_V4: &[&str] = &["8.8.8.8", "1.1.1.1"];

/// Addresses the DPI on the ISP link injects instead of the real answer for
/// some names.
pub(super) const REFERENCE_STUBS: &[Ipv4Addr] =
    &[Ipv4Addr::new(8, 6, 112, 0), Ipv4Addr::new(8, 47, 69, 0)];

pub(super) const QTYPE_A: u16 = 1;
pub(super) const QTYPE_AAAA: u16 = 28;

pub(super) const CHOICE_TTL: Duration = Duration::from_secs(5 * 60);
pub(super) const RACE_BUDGET: Duration = Duration::from_millis(700);
pub(super) const QUERY_TIMEOUT: Duration = Duration::from_secs(4);
pub(super) const FAST_PATH_TIMEOUT: Duration = Duration::from_millis(250);
pub(super) const DOH_FAST_TIMEOUT: Duration = Duration::from_millis(600);
pub(super) const PREFERRED_GRACE: Duration = Duration::from_millis(500);

pub(super) const PREFERRED_NAMES: &[&str] = &[
    "cloudcode-pa.googleapis.com",
    "daily-cloudcode-pa.googleapis.com",
];

pub const ANSWER_TTL: Duration = Duration::from_secs(30);
pub(super) const ANSWER_STALE_TTL: Duration = Duration::from_secs(5 * 60);
pub(super) const ANSWER_IDLE: Duration = Duration::from_secs(10 * 60);
pub(super) const PASSTHROUGH_TTL: u32 = 60;

pub(super) const CONTROL_NAMES: &[&str] = &[
    "chatgpt.com",
    "api.openai.com",
    "claude.ai",
    "gemini.google.com",
    "ai.google.dev",
];

const LEAD_PROVIDER: &str = "dns-ai.ru";

pub fn all_v6() -> Vec<&'static str> {
    let mut list: Vec<&'static str> = in_user_order()
        .into_iter()
        .filter(|p| !denied(p.name))
        .flat_map(|p| p.v6.iter().copied())
        .collect();
    if !crate::settings::rotation_enabled() {
        list.truncate(1);
    }
    list
}

pub fn fallback_v4() -> Vec<&'static str> {
    let mut list: Vec<&'static str> = in_user_order()
        .into_iter()
        .filter(|p| !denied(p.name))
        .filter_map(|p| p.v4.first().copied())
        .collect();
    if !crate::settings::rotation_enabled() {
        list.truncate(1);
    }
    list
}

pub(super) fn in_user_order() -> Vec<&'static Provider> {
    let wanted = crate::settings::provider_order_cached();
    let mut out: Vec<&'static Provider> = Vec::new();
    let user_placed_lead = wanted.iter().any(|n| n.eq_ignore_ascii_case(LEAD_PROVIDER));
    if !user_placed_lead {
        if let Some(p) = PROVIDERS.iter().find(|p| p.name == LEAD_PROVIDER) {
            out.push(p);
        }
    }
    for name in &wanted {
        if let Some(p) = PROVIDERS.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
            if !out.iter().any(|q| q.name == p.name) {
                out.push(p);
            }
        }
    }
    for p in PROVIDERS {
        if !out.iter().any(|q| q.name == p.name) {
            out.push(p);
        }
    }
    out
}

pub fn udp_provider_names() -> Vec<&'static str> {
    in_user_order()
        .into_iter()
        .filter(|p| !p.v4.is_empty())
        .map(|p| p.name)
        .collect()
}

pub fn ordered_provider_names() -> Vec<&'static str> {
    in_user_order().into_iter().map(|p| p.name).collect()
}

pub(super) fn denied(name: &str) -> bool {
    crate::settings::disabled_providers_cached()
        .iter()
        .any(|d| d.eq_ignore_ascii_case(name))
}

pub(super) fn provider_disabled(name: &str) -> bool {
    if denied(name) {
        return true;
    }
    if !crate::settings::rotation_enabled() {
        return only_provider() != Some(name);
    }
    false
}

pub(super) fn only_provider() -> Option<&'static str> {
    let disabled = crate::settings::disabled_providers_cached();
    in_user_order()
        .into_iter()
        .map(|p| p.name)
        .find(|n| !disabled.iter().any(|d| d.eq_ignore_ascii_case(n)))
}

pub(super) fn ask_provider(
    provider: &Provider,
    query: &[u8],
    if_index: u32,
    timeout: Duration,
) -> Option<Vec<u8>> {
    if provider_disabled(provider.name) {
        return None;
    }
    match provider.transport {
        Transport::Udp => {
            for server in provider.v4 {
                let Ok(ip) = server.parse::<Ipv4Addr>() else {
                    continue;
                };
                if let Ok(reply) = dns_client::query_raw_via(query, ip, if_index, timeout) {
                    return Some(reply);
                }
            }
            None
        }
        Transport::Doh(endpoint) => {
            let mut reply = crate::doh::query(endpoint, query, timeout).ok()?;
            if reply.len() >= 2 && query.len() >= 2 {
                reply[0] = query[0];
                reply[1] = query[1];
            }
            Some(reply)
        }
    }
}

pub(super) fn preferred_provider(name: &str) -> Option<usize> {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    if !PREFERRED_NAMES.contains(&name.as_str()) {
        return None;
    }
    PROVIDERS
        .iter()
        .position(|p| matches!(p.transport, Transport::Doh(_)))
}

pub(super) fn one_shot_floor(provider: &Provider, budget: Duration) -> Duration {
    match provider.transport {
        Transport::Doh(_) => budget.max(DOH_FAST_TIMEOUT),
        Transport::Udp => budget,
    }
}

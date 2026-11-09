use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use crate::dns_client;
use super::engine::*;
use super::providers::*;
use super::verdict::*;



    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(s.parse().unwrap())
    }

    #[test]
    fn served_verdicts_are_kept_per_name_case_folded() {
        note_served("Daily-Cloudcode-PA.googleapis.com.", Verdict::Substituted);
        assert_eq!(
            served_verdict("daily-cloudcode-pa.googleapis.com"),
            Some(Verdict::Substituted)
        );
        assert_eq!(served_verdict("nobody.asked.example"), None);
    }

    #[test]
    fn forgetting_a_name_drops_its_choice_and_its_answers() {
        let _cache = clear_answers();
        let name = "forget-me.example";
        remember_choice((name.to_string(), QTYPE_A), 0, Verdict::Substituted);
        assert!(cached_choice(&(name.to_string(), QTYPE_A)).is_some());
        let query = dns_client::build_query(name, 0x1111);
        let mut reply = query.clone();
        reply[2] = 0x81;
        reply[3] = 0x80;
        reply[7] = 1;
        reply.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 10, 0, 0, 1]);
        remember_answer(&query, &reply, "test", Verdict::Substituted, true);
        assert!(cached_answer(&query, false, false).is_some());

        forget_names(&["FORGET-ME.example"]);
        assert!(cached_choice(&(name.to_string(), QTYPE_A)).is_none());
        assert!(cached_answer(&query, false, false).is_none());
        assert!(
            cached_answer(&query, true, false).is_none(),
            "not served as a stale substitution either"
        );
        // The shape survives for the warm loop to replay.
        let shapes = warm_queries(&[name]);
        assert!(
            shapes.iter().any(|q| answer_key(q) == answer_key(&query)),
            "the client's query shape must still be warmed"
        );
    }

    #[test]
    fn only_an_a_answer_records_a_served_verdict() {
        let name = "served-a.example";
        let a = dns_client::build_query(name, 1);
        let mut aaaa = a.clone();
        let n = aaaa.len();
        aaaa[n - 3] = 28; // qtype AAAA
        let hit = Some((Vec::new(), "test", Verdict::Substituted));
        note_served_a(&a, &hit);
        assert_eq!(served_verdict(name), Some(Verdict::Substituted));
        let hit = Some((Vec::new(), "test", Verdict::Passthrough));
        note_served_a(&aaaa, &hit);
        assert_eq!(
            served_verdict(name),
            Some(Verdict::Substituted),
            "an AAAA passthrough must not overwrite the A verdict"
        );
    }

    fn udp_providers() -> Vec<&'static Provider> {
        PROVIDERS
            .iter()
            .filter(|p| matches!(p.transport, Transport::Udp))
            .collect()
    }

    #[test]
    fn every_udp_provider_has_at_least_one_address() {
        assert!(!PROVIDERS.is_empty());
        for p in udp_providers() {
            assert!(!p.v4.is_empty(), "{} has no IPv4 address", p.name);
            for a in p.v4 {
                assert!(a.parse::<Ipv4Addr>().is_ok(), "{} is not an address", a);
            }
        }
    }

    /// The NRPT rule takes one address per provider, so a provider outage falls
    /// through to a different service rather than to its own dead sibling.
    #[test]
    fn the_fallback_list_is_one_address_per_udp_provider() {
        let fallback = fallback_v4();
        let udp = udp_providers();
        assert_eq!(fallback.len(), udp.len());
        for (i, p) in udp.iter().enumerate() {
            assert_eq!(fallback[i], p.v4[0]);
        }
    }

    /// The invariant a DoH provider lives or dies by.
    ///
    /// `fallback_v4`/`all_v6` feed NRPT nameserver lists, and Windows can only be
    /// pointed at something that answers UDP:53. A DoH endpoint answers nothing
    /// there - measured: both of dns-ai.ru's addresses time out on 53 - so if one
    /// ever reached a rule, that name would get a nameserver that never replies
    /// and the whole rule would hang on it. Enforced structurally by empty
    /// `v4`/`v6`, which is easy to undo by accident while "filling in" a provider
    /// entry, so it is asserted rather than commented.
    #[test]
    fn a_doh_provider_is_never_offered_as_a_nameserver() {
        let listed: Vec<&str> = fallback_v4().into_iter().chain(all_v6()).collect();
        for p in PROVIDERS {
            let Transport::Doh(endpoint) = p.transport else {
                continue;
            };
            assert!(
                p.v4.is_empty() && p.v6.is_empty(),
                "{} speaks DoH, so its address list must stay empty",
                p.name
            );
            for a in endpoint.addrs {
                assert!(
                    !listed.contains(a),
                    "{} would be handed to Windows as a nameserver",
                    a
                );
            }
        }
    }

    /// The flag defaults to "no tunnel", which is the behaviour every existing
    /// machine already has: a relay that never learns the answer keeps
    /// substituting exactly as before rather than quietly standing down.
    ///
    /// The region-400 override lives in the same test because it shares this
    /// flag: while forced, the relay substitutes even though the tunnel verdict
    /// says stand down, and the force expires on its own.
    #[test]
    fn the_vpn_flag_defaults_to_off_and_round_trips() {
        assert!(!vpn_is_active(), "должно быть выключено по умолчанию");
        set_vpn_active(true);
        assert!(vpn_is_active());

        force_substitution(Duration::from_millis(80));
        assert!(substitution_forced());
        assert!(!vpn_is_active(), "forced: the tunnel no longer decides");
        assert!(tunnel_carries_client(), "the raw verdict is still readable");
        std::thread::sleep(Duration::from_millis(120));
        assert!(!substitution_forced());
        assert!(
            vpn_is_active(),
            "the window closed; the tunnel decides again"
        );

        set_vpn_active(false);
        assert!(!vpn_is_active());
    }

    /// Preference is for the two gate names and nothing else. Widening it would
    /// put a bounded wait on names that never needed one.
    #[test]
    fn only_the_gate_names_have_a_preferred_provider() {
        let doh = PROVIDERS
            .iter()
            .position(|p| matches!(p.transport, Transport::Doh(_)))
            .expect("a DoH provider");
        assert_eq!(preferred_provider("cloudcode-pa.googleapis.com"), Some(doh));
        assert_eq!(
            preferred_provider("daily-cloudcode-pa.googleapis.com"),
            Some(doh)
        );
        // A question name may arrive rooted or in any case; neither changes it.
        assert_eq!(
            preferred_provider("CloudCode-PA.GoogleAPIs.com."),
            Some(doh)
        );
        for other in [
            "generativelanguage.googleapis.com",
            "jetski-webchannel.googleapis.com",
            "accounts.google.com",
            "",
        ] {
            assert_eq!(preferred_provider(other), None, "{}", other);
        }
    }

    /// 250 ms cannot cover a TLS handshake, so a cached choice pointing at the
    /// DoH provider would fail on the fast path every time, be forgotten, and
    /// re-race - the fast path becoming the slow one.
    #[test]
    fn the_fast_path_gives_a_doh_provider_room_to_answer() {
        let doh = PROVIDERS
            .iter()
            .find(|p| matches!(p.transport, Transport::Doh(_)))
            .expect("a DoH provider");
        let udp = PROVIDERS
            .iter()
            .find(|p| matches!(p.transport, Transport::Udp))
            .expect("a UDP provider");
        assert_eq!(one_shot_floor(doh, FAST_PATH_TIMEOUT), DOH_FAST_TIMEOUT);
        assert_eq!(one_shot_floor(udp, FAST_PATH_TIMEOUT), FAST_PATH_TIMEOUT);
        // A caller that already budgeted more keeps it.
        assert_eq!(one_shot_floor(doh, QUERY_TIMEOUT), QUERY_TIMEOUT);
        // And the whole thing still fits inside the second Windows waits (I23).
        assert!(DOH_FAST_TIMEOUT < Duration::from_secs(1));
        assert!(PREFERRED_GRACE <= RACE_BUDGET);
    }

    /// A DoH endpoint's addresses still have to be addresses: they are dialled
    /// directly, precisely so the resolver is reachable before DNS works.
    #[test]
    fn doh_endpoints_carry_parsable_addresses() {
        for p in PROVIDERS {
            if let Transport::Doh(endpoint) = p.transport {
                assert!(!endpoint.addrs.is_empty(), "{} has no address", p.name);
                for a in endpoint.addrs {
                    assert!(a.parse::<IpAddr>().is_ok(), "{} is not an address", a);
                }
                assert!(endpoint.path.starts_with('/'), "path must be absolute");
            }
        }
    }

    /// A reference resolver must never be a provider: it exists to show what an
    /// unsubstituted answer looks like.
    #[test]
    fn reference_resolvers_are_not_providers() {
        let providers: Vec<&str> = PROVIDERS
            .iter()
            .flat_map(|p| p.v4.iter().copied())
            .collect();
        for r in REFERENCE_V4 {
            assert!(!providers.contains(r), "{} is on both lists", r);
        }
    }

    /// The proxy addresses the three providers were measured handing out. In
    /// production these are learned at runtime from `CONTROL_NAME`; here they
    /// stand in for what that probe would have found.
    fn learned_proxies() -> Vec<IpAddr> {
        vec![
            v4("87.228.47.204"),
            v4("45.88.174.254"),
            v4("37.230.192.51"),
        ]
    }

    #[test]
    fn the_same_netblock_is_a_passthrough() {
        // What xbox-dns now answers for cloudcode-pa: Google's own edge, a
        // different host inside the same /16 the reference gave.
        let candidate = [v4("172.217.113.4")];
        let reference = [v4("172.217.114.4"), v4("172.217.112.4")];
        assert_eq!(
            classify(&candidate, &reference, &learned_proxies()),
            Verdict::Passthrough
        );
    }

    #[test]
    fn a_proxy_address_is_a_substitution() {
        let google = [v4("172.217.118.4")];
        for proxy in learned_proxies() {
            assert_eq!(
                classify(&[proxy], &google, &learned_proxies()),
                Verdict::Substituted,
                "{:?} is a measured proxy address",
                proxy
            );
        }
    }

    /// The false positive the proxy set exists to kill: two different Google
    /// netblocks are not a substitution. Nothing about the addresses themselves
    /// says so - only knowing what the provider's proxy looks like does.
    #[test]
    fn another_google_netblock_is_only_a_sibling() {
        assert_eq!(
            classify(
                &[v4("173.194.220.95")],
                &[v4("64.233.163.95")],
                &learned_proxies()
            ),
            Verdict::Sibling
        );
        assert!(rank(Verdict::Sibling) < rank(Verdict::Substituted));
    }

    /// Before the control probe has ever run there is nothing to recognise a
    /// proxy by, so "differs from the honest answer" is taken at face value -
    /// the behaviour the tool had before, which never prefers a passthrough.
    #[test]
    fn without_a_learned_proxy_any_difference_counts() {
        assert_eq!(
            classify(&[v4("173.194.220.95")], &[v4("64.233.163.95")], &[]),
            Verdict::Substituted
        );
        // Still not enough to call an identical answer substituted.
        assert_eq!(
            classify(&[v4("172.217.113.4")], &[v4("172.217.114.4")], &[]),
            Verdict::Passthrough
        );
    }

    /// A DPI-stubbed reference must not turn every answer into a substitution.
    #[test]
    fn a_stubbed_reference_is_no_reference() {
        let stub = [v4("8.6.112.0"), v4("8.47.69.0")];
        assert_eq!(
            classify(&[v4("172.217.118.4")], &stub, &learned_proxies()),
            Verdict::Unknown
        );
        assert_eq!(
            classify(&[v4("87.228.47.204")], &stub, &learned_proxies()),
            Verdict::Unknown
        );
    }

    #[test]
    fn nothing_to_compare_is_unknown() {
        assert_eq!(classify(&[], &[v4("8.8.8.8")], &[]), Verdict::Unknown);
        assert_eq!(classify(&[v4("87.228.47.204")], &[], &[]), Verdict::Unknown);
        // An AAAA answer against an A-only reference says nothing.
        let v6: IpAddr = "2a00:ab00:1233:26::50".parse::<IpAddr>().unwrap();
        assert_eq!(
            classify(&[v6], &[v4("172.217.118.4")], &[]),
            Verdict::Unknown
        );
    }

    #[test]
    fn a_substitution_outranks_everything_else() {
        assert!(rank(Verdict::Substituted) > rank(Verdict::Unknown));
        assert!(rank(Verdict::Unknown) > rank(Verdict::Sibling));
        assert!(rank(Verdict::Sibling) > rank(Verdict::Passthrough));
    }

    /// The reason the whole module exists: the answer that broke the tool must
    /// be recognised as not-a-substitution, so the next provider gets a turn.
    #[test]
    fn the_regression_that_broke_the_tool_is_detected() {
        let xbox_cloudcode = [v4("172.217.113.4")];
        let google = [v4("172.217.114.4")];
        assert_ne!(
            classify(&xbox_cloudcode, &google, &learned_proxies()),
            Verdict::Substituted
        );
    }

    /// The union is what makes a rotating provider recognisable: geohide serves
    /// its proxy from three addresses in three unrelated /16s, so one probe only
    /// ever sees part of the set.
    #[test]
    fn learned_proxy_addresses_accumulate() {
        let idx = PROVIDERS.len() + 41; // a slot no real provider uses
        learn_proxy_addrs(idx, vec![v4("95.182.120.241")]);
        learn_proxy_addrs(idx, vec![v4("37.230.192.51"), v4("95.182.120.241")]);
        let known = known_proxy_addrs(idx);
        assert_eq!(known.len(), 2, "duplicates must not pile up: {:?}", known);
        assert!(known.contains(&v4("37.230.192.51")));

        // An answer served from the address the first probe missed is still
        // recognised as that provider's proxy.
        assert_eq!(
            classify(&[v4("37.230.192.51")], &[v4("172.217.118.4")], &known),
            Verdict::Substituted
        );
    }

    /// The control names are the one place this tool announces itself to a
    /// provider, and the source is public. Several, from unrelated operators,
    /// so no single upstream change - or a rule aimed at this tool - takes the
    /// whole learning mechanism out.
    #[test]
    fn control_names_are_several_and_not_one_operator() {
        assert!(
            CONTROL_NAMES.len() >= 3,
            "one control name is a signature, not a probe"
        );
        let non_google = CONTROL_NAMES
            .iter()
            .filter(|n| !n.contains("google."))
            .count();
        assert!(
            non_google >= 2,
            "the set must survive a Google-side change: {:?}",
            CONTROL_NAMES
        );
        // The DPI on the ISP link stubs this one, so its reference is unusable.
        assert!(!CONTROL_NAMES.contains(&"grok.com"));
    }

    /// An empty learning round must not overwrite what is already known, or a
    /// provider that failed to answer the control probe once would lose its set.
    #[test]
    fn an_empty_probe_does_not_erase_the_set() {
        let idx = PROVIDERS.len() + 42;
        learn_proxy_addrs(idx, vec![v4("87.228.47.204")]);
        learn_proxy_addrs(idx, vec![]);
        assert_eq!(known_proxy_addrs(idx), vec![v4("87.228.47.204")]);
    }

    /// Live: what the NRPT fallback list will be built from for each routed
    /// name. `daily-cloudcode-pa` must come back with geohide only - the bug
    /// was xbox-dns/comss (which return genuine Google for it) sitting in that
    /// list and leaking. Run VPN-off.
    #[test]
    #[ignore = "needs a live network, VPN off; run with --ignored"]
    fn substituting_addrs_excludes_passthrough_providers() {
        for name in [
            "cloudcode-pa.googleapis.com",
            "daily-cloudcode-pa.googleapis.com",
            "generativelanguage.googleapis.com",
            "antigravity-unleash.goog",
        ] {
            let subs = substituting_addrs(name, 0);
            println!("{:<38} substituted by {:?}", name, subs);
            // Whatever is returned must be a real provider address, never a
            // reference resolver - a reference in the fallback would be a leak.
            for s in &subs {
                assert!(
                    !REFERENCE_V4.contains(s),
                    "{} is a reference resolver, not a substituter",
                    s
                );
            }
        }
    }

    /// Live: the whole pool, provider by provider and name by name — what each
    /// answered, what proxy set was learned for it, and the verdict that follows.
    ///
    /// This is the quarterly re-measurement P14 asks for, in one command. It
    /// asserts nothing about *who* substitutes what, because that is exactly the
    /// thing that changes without warning and must not be pinned in a test (N15:
    /// a measurement recorded as a property of a host outlived the measurement by
    /// months). It only pins the two rules that are ours: an NRPT fallback never
    /// names a reference resolver, and never names a DoH provider (I49).
    ///
    ///     cargo test the_pool_measured_provider_by_provider -- --ignored --nocapture
    #[test]
    #[ignore = "needs a live network, VPN off; run with --ignored"]
    fn the_pool_measured_provider_by_provider() {
        const NAMES: &[&str] = &[
            "cloudcode-pa.googleapis.com",
            "daily-cloudcode-pa.googleapis.com",
            "generativelanguage.googleapis.com",
        ];

        ensure_proxy_sets_learned(0);
        println!("learned proxy sets:");
        for (idx, p) in PROVIDERS.iter().enumerate() {
            println!("  {:<12} {:?}", p.name, known_proxy_addrs(idx));
        }

        for name in NAMES {
            let query = dns_client::build_query(name, 0x6767);
            let mut reference: Vec<IpAddr> = Vec::new();
            for server in REFERENCE_V4 {
                if let Ok(ip) = server.parse::<Ipv4Addr>() {
                    if let Ok(reply) = dns_client::query_raw_via(&query, ip, 0, QUERY_TIMEOUT) {
                        reference.extend(dns_client::answer_addrs(&reply));
                    }
                }
            }
            println!("\n{}\n  reference {:?}", name, reference);
            for (idx, p) in PROVIDERS.iter().enumerate() {
                let known = known_proxy_addrs(idx);
                match ask_provider(p, &query, 0, QUERY_TIMEOUT) {
                    None => println!("  {:<12} нет ответа", p.name),
                    Some(reply) => {
                        let addrs = dns_client::answer_addrs(&reply);
                        // Wire counts alongside the parsed addresses: an empty
                        // list with ANCOUNT>0 is a parse problem, an empty list
                        // with ANCOUNT=0 is the provider (G24 was the first).
                        let rcode = reply.get(3).map_or(0xFF, |b| b & 0x0F);
                        let ancount = if reply.len() >= 8 {
                            u16::from_be_bytes([reply[6], reply[7]])
                        } else {
                            0
                        };
                        // Both verdicts: the bootstrap one the relay may act on,
                        // and the stricter one an NRPT fallback needs.
                        println!(
                            "  {:<12} rcode={} an={} len={} {:?} -> race {:?}, rule {}",
                            p.name,
                            rcode,
                            ancount,
                            reply.len(),
                            addrs,
                            classify(&addrs, &reference, &known),
                            if known.is_empty() {
                                "исключён (прокси-набор не выучен)".to_string()
                            } else {
                                format!("{:?}", classify(&addrs, &reference, &known))
                            }
                        );
                    }
                }
            }
            for s in substituting_addrs(name, 0) {
                assert!(!REFERENCE_V4.contains(&s), "{} is a reference resolver", s);
                assert!(
                    PROVIDERS
                        .iter()
                        .any(|p| p.v4.contains(&s) && !matches!(p.transport, Transport::Doh(_))),
                    "{} is not a UDP provider address (I49)",
                    s
                );
            }
        }
    }

    /// Live end-to-end: races the real providers and prints what each answered.
    /// Must run with the VPN off, or every provider sees a foreign client and
    /// substitutes nothing.
    #[test]
    #[ignore = "needs a live network; run with --ignored"]
    fn races_the_real_providers() {
        for name in [
            "generativelanguage.googleapis.com",
            "cloudcode-pa.googleapis.com",
        ] {
            let mut query = vec![0x77, 0x01, 0x01, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
            for label in name.split('.') {
                query.push(label.len() as u8);
                query.extend_from_slice(label.as_bytes());
            }
            query.extend_from_slice(&[0, 0x00, 0x01, 0x00, 0x01]);

            match resolve_best(&query, 0) {
                Some((reply, provider, verdict)) => {
                    println!(
                        "{:<38} {:?} via {} -> {:?}",
                        name,
                        verdict,
                        provider,
                        dns_client::answer_addrs(&reply)
                    );
                }
                None => println!("{:<38} no provider answered", name),
            }
        }
    }

    /// Header + question for `name`, shaped the way a real client asks.
    fn query_for(name: &str, id: u16, edns: bool) -> Vec<u8> {
        let mut q = vec![0, 0, 0x01, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
        q[0..2].copy_from_slice(&id.to_be_bytes());
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.extend_from_slice(&[0, 0x00, 0x01, 0x00, 0x01]);
        if edns {
            q[11] = 1; // ARCOUNT
            q.extend_from_slice(&[0, 0x00, 0x29, 0x10, 0x00, 0, 0, 0, 0, 0x00, 0x00]);
        }
        q
    }

    /// One A record for `name`, TTL 60, exactly the shape geohide answers with.
    fn reply_for(name: &str, id: u16, addr: [u8; 4]) -> Vec<u8> {
        let mut b = query_for(name, id, false);
        b[2] = 0x81;
        b[3] = 0x80;
        b[7] = 1; // ANCOUNT
        b.extend_from_slice(&[0xC0, 0x0C, 0x00, 0x01, 0x00, 0x01, 0, 0, 0, 60, 0x00, 0x04]);
        b.extend_from_slice(&addr);
        b
    }

    /// Serialises every test that shares the process-wide answer cache.
    ///
    /// This used to be a bare call, and cargo runs tests in parallel threads of
    /// one process: two of them would each wipe the map the other had just
    /// written, and whichever asserted second failed. Roughly one run in ten, and
    /// it read exactly like a product bug. Holding the guard for the length of a
    /// test is what makes "nothing else may be in the cache" true rather than
    /// merely intended.
    static CACHE_TESTS: Mutex<()> = Mutex::new(());

    #[must_use = "hold the guard for the whole test, or the cache is shared again"]
    fn clear_answers() -> std::sync::MutexGuard<'static, ()> {
        // A panicking test poisons this, and the next one still has to run: what
        // it guards is wiped by every caller anyway, so the poison carries no
        // information worth propagating.
        let guard = CACHE_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(mut g) = ANSWERS.lock() {
            *g = None;
        }
        guard
    }

    #[test]
    fn a_cached_answer_is_restamped_for_the_client_that_asks() {
        let _cache = clear_answers();
        let name = "cache-restamp.example";
        let stored = reply_for(name, 0x1111, [45, 155, 204, 190]);
        remember_answer(
            &query_for(name, 0x1111, false),
            &stored,
            "geohide.ru",
            Verdict::Substituted,
            true,
        );

        // A different client, a different transaction id, the same question.
        let asking = query_for(name, 0xBEEF, false);
        let (reply, provider, verdict) = cached_answer(&asking, false, true).unwrap();
        assert_eq!(&reply[0..2], &[0xBE, 0xEF]);
        assert_eq!(provider, "geohide.ru");
        assert_eq!(verdict, Verdict::Substituted);
        assert_eq!(
            dns_client::answer_addrs(&reply),
            dns_client::answer_addrs(&stored)
        );
    }

    /// A reply is only an answer to the exact question that fetched it. Windows
    /// sends EDNS0; a cache keyed loosely enough to serve that reply to a query
    /// without an OPT record would be handing back a malformed message.
    #[test]
    fn a_differently_shaped_question_is_a_miss() {
        let _cache = clear_answers();
        let name = "cache-shape.example";
        remember_answer(
            &query_for(name, 1, false),
            &reply_for(name, 1, [1, 2, 3, 4]),
            "geohide.ru",
            Verdict::Substituted,
            true,
        );
        assert!(cached_answer(&query_for(name, 2, false), false, true).is_some());
        assert!(cached_answer(&query_for(name, 2, true), false, true).is_none());
    }

    /// Serving a stored reply with its original TTL would keep Windows on it for
    /// the cache window *plus* the resolver's TTL, i.e. past the minute in which
    /// these providers may move the address.
    #[test]
    fn a_served_answer_carries_the_time_it_already_spent_in_the_cache() {
        let _cache = clear_answers();
        let name = "cache-ttl.example";
        let query = query_for(name, 3, false);
        remember_answer(
            &query,
            &reply_for(name, 3, [1, 2, 3, 4]),
            "geohide.ru",
            Verdict::Substituted,
            true,
        );
        if let Ok(mut g) = ANSWERS.lock() {
            let entry = g.as_mut().unwrap().get_mut(&answer_key(&query).unwrap());
            let entry = entry.unwrap();
            entry.at = Instant::now() - Duration::from_secs(20);
        }
        let (reply, _, _) = cached_answer(&query, false, true).unwrap();
        assert_eq!(dns_client::answer_ttl(&reply), Some(40));
    }

    /// Only a substitution is worth serving stale. Keeping a passthrough alive
    /// would hold the region gate shut, and there the NRPT fallback - built from
    /// measured substituters (I22) - is the better thing to fall through to.
    #[test]
    fn only_a_substitution_survives_past_its_freshness() {
        let _cache = clear_answers();
        for (name, verdict, expected) in [
            ("stale-sub.example", Verdict::Substituted, true),
            ("stale-pass.example", Verdict::Passthrough, false),
        ] {
            let query = query_for(name, 4, false);
            remember_answer(
                &query,
                &reply_for(name, 4, [1, 2, 3, 4]),
                "geohide.ru",
                verdict,
                true,
            );
            if let Ok(mut g) = ANSWERS.lock() {
                let key = answer_key(&query).unwrap();
                g.as_mut().unwrap().get_mut(&key).unwrap().at =
                    Instant::now() - ANSWER_TTL - Duration::from_secs(5);
            }
            assert!(cached_answer(&query, false, true).is_none(), "{}", name);
            assert_eq!(
                cached_answer(&query, true, true).is_some(),
                expected,
                "{}",
                name
            );
        }
    }

    /// The whole point of warming: it has to re-ask the shape the client sends,
    /// or every real query still goes upstream while the cache serves nobody.
    #[test]
    fn warming_replays_the_shapes_a_client_actually_sent() {
        let _cache = clear_answers();
        let name = "warm-shape.example";
        let asked = query_for(name, 0x4242, true);
        remember_answer(
            &asked,
            &reply_for(name, 0x4242, [1, 2, 3, 4]),
            "geohide.ru",
            Verdict::Substituted,
            true,
        );

        let queries = warm_queries(&[name]);
        assert_eq!(queries.len(), 1, "the seen shape, and no invented one");
        assert_eq!(answer_key(&queries[0]), answer_key(&asked));
    }

    /// A cached "alive" must not survive the address dying. Measured: geohide
    /// rotated which of its three was dead three times inside an hour, and a
    /// ten-minute verdict meant the relay advertised a corpse for nine of them.
    #[test]
    fn the_warm_path_re_probes_instead_of_trusting_what_it_knows() {
        let addr = v4("203.0.113.9");
        // A blackhole address in TEST-NET-3: nothing there, so a probe says dead.
        remember_liveness(addr, true);
        assert_eq!(cached_liveness(&addr), Some(true));

        // The client path takes the cached word for it and stays cheap...
        assert!(dead_addrs(&[addr], false, None).is_empty());
        // ...while the warm path asks the network and finds out otherwise.
        assert_eq!(dead_addrs(&[addr], true, None), vec![addr]);
    }

    /// Believing "alive" for longer than the providers keep an address alive is
    /// the whole bug; the warm loop has to get at least one pass inside it.
    #[test]
    fn a_live_verdict_expires_faster_than_the_addresses_rotate() {
        assert!(LIVENESS_TTL_ALIVE <= Duration::from_secs(30));
    }

    /// The outage this exists to stop: comss answers `daily-cloudcode-pa` with
    /// the genuine Google address and a TTL of 3199 s. Passed through unchanged,
    /// one lost race pins Windows to a blocked address for 53 minutes.
    #[test]
    fn an_answer_that_did_not_beat_the_gate_expires_quickly() {
        let mut long = reply_for("ttl-cap.example", 5, [172, 217, 115, 4]);
        let at = long.len() - 10;
        long[at..at + 4].copy_from_slice(&3199u32.to_be_bytes());
        assert_eq!(dns_client::answer_ttl(&long), Some(3199));

        for v in [Verdict::Passthrough, Verdict::Sibling, Verdict::Unknown] {
            let out = vet(long.clone(), v, false, None);
            assert_eq!(
                dns_client::answer_ttl(&out),
                Some(PASSTHROUGH_TTL),
                "{:?}",
                v
            );
        }
        // A substitution keeps the resolver's own TTL - it is the answer we want
        // the client to hold, and these providers already publish a short one.
        let subbed = vet(long.clone(), Verdict::Substituted, false, None);
        assert_eq!(dns_client::answer_ttl(&subbed), Some(3199));
    }

    /// A genuine answer must not evict, nor outrank, a substitution we already
    /// hold: that is the difference between one lost race and an hour of them.
    #[test]
    fn a_substitution_in_hand_beats_a_fresh_genuine_answer() {
        let _cache = clear_answers();
        let name = "prefer-sub.example";
        let query = query_for(name, 1, false);
        let good = reply_for(name, 1, [45, 155, 204, 190]);
        remember_answer(&query, &good, "geohide.ru", Verdict::Substituted, true);

        let genuine = reply_for(name, 1, [172, 217, 115, 4]);
        remember_answer(&query, &genuine, "comss.one", Verdict::Passthrough, true);

        // The cache still holds the substitution...
        let (cached, provider, verdict) = cached_answer(&query, false, true).unwrap();
        assert_eq!(provider, "geohide.ru");
        assert_eq!(verdict, Verdict::Substituted);
        assert_eq!(
            dns_client::answer_addrs(&cached),
            dns_client::answer_addrs(&good)
        );

        // ...and it is what goes out, not the genuine answer that just arrived.
        let fresh = (genuine, "comss.one", Verdict::Passthrough);
        let (out, who, v) = prefer_substituted(&query, true, fresh).unwrap();
        assert_eq!((who, v), ("geohide.ru", Verdict::Substituted));
        assert_eq!(
            dns_client::answer_addrs(&out),
            dns_client::answer_addrs(&good)
        );
    }

    /// With nothing better in hand the genuine answer is still served: refusing
    /// to answer would only push Windows onto the unvetted NRPT fallback.
    #[test]
    fn a_genuine_answer_is_served_when_there_is_nothing_better() {
        let _cache = clear_answers();
        let name = "no-sub.example";
        let query = query_for(name, 2, false);
        let genuine = reply_for(name, 2, [172, 217, 115, 4]);
        let fresh = (genuine, "comss.one", Verdict::Passthrough);
        let (_, who, v) = prefer_substituted(&query, true, fresh).unwrap();
        assert_eq!((who, v), ("comss.one", Verdict::Passthrough));
    }

    /// Before any client has asked, there is no shape to replay - warming still
    /// has to prime the name, or the first query pays the full cold price.
    #[test]
    fn an_unseen_name_is_warmed_with_a_plain_query() {
        let _cache = clear_answers();
        let queries = warm_queries(&["warm-unseen.example"]);
        assert_eq!(queries.len(), 1);
        assert_eq!(
            dns_client::question_name(&queries[0]).as_deref(),
            Some("warm-unseen.example")
        );
    }

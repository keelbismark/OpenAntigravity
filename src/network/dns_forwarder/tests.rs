    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn a_gate_hosts_aaaa_and_https_questions_get_an_empty_answer() {
        for qtype in [28u8, 64, 65] {
            let mut q = dns_client::build_query("daily-cloudcode-pa.googleapis.com", 9);
            let n = q.len();
            q[n - 3] = qtype;
            let r = own_answer(&q, "daily-cloudcode-pa.googleapis.com").expect("answered");
            assert!(dns_client::answer_addrs(&r).is_empty(), "qtype {qtype}");
            assert_eq!(dns_client::question_type(&r), Some(qtype as u16));
        }
        // Not a gate host: relayed as before.
        let mut q = dns_client::build_query("storage.googleapis.com", 9);
        let n = q.len();
        q[n - 3] = 28;
        assert!(own_answer(&q, "storage.googleapis.com").is_none());
    }

    #[test]
    fn the_network_fingerprint_is_never_the_unset_value_and_tells_networks_apart() {
        let a = network_fingerprint(27, Some(27), false);
        let b = network_fingerprint(27, Some(55), true);
        let c = network_fingerprint(27, Some(27), true);
        assert_ne!(a, 0);
        assert_ne!(network_fingerprint(0, None, false), 0);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, network_fingerprint(27, Some(27), false));
    }

    #[test]
    fn a_penalty_reads_in_minutes_or_hours() {
        assert_eq!(human_minutes(Duration::from_secs(600)), "10 РјРёРЅ");
        assert_eq!(human_minutes(Duration::from_secs(3600)), "1 С‡");
        assert_eq!(human_minutes(Duration::from_secs(6 * 3600)), "6 С‡");
        assert_eq!(human_minutes(Duration::from_secs(90 * 60)), "90 РјРёРЅ");
    }

    /// Everything the version check does hangs off this: a relay that predates
    /// versioning leaves no file, and it must read as older than this build
    /// rather than as "no relay" or as a parse error nobody handles.
    #[test]
    fn an_unreadable_version_is_the_oldest_one() {
        assert_eq!(parse_version(None), 0);
        assert_eq!(parse_version(Some("")), 0);
        assert_eq!(parse_version(Some("РЅРµ С‡РёСЃР»Рѕ")), 0);
        assert_eq!(parse_version(Some(" 7 \r\n")), 7);
        assert!(parse_version(None) < RELAY_VERSION);
    }

    /// The warm loop only exists to beat the ~1 s Windows waits before asking
    /// the next NRPT nameserver, so it has to refresh well inside the window the
    /// answer cache serves from.
    #[test]
    fn warming_runs_more_often_than_an_answer_goes_stale() {
        assert!(WARM_EVERY < resolvers::ANSWER_TTL);
    }

    /// A burst of one message must not eat the sixty lines a report shows.
    /// The first of a run goes out at once; the rest are counted and the count
    /// is written the moment anything else is logged.
    #[test]
    fn a_run_of_one_line_is_collapsed_into_a_count() {
        let line = "РЅР°РїСЂСЏРјСѓСЋ X: РёРјСЏ РЅРµ СЂР°Р·СЂРµС€Р°РµС‚СЃСЏ";
        // The first is written, with nothing held back before it.
        assert_eq!(collapse_repeat(line), (None, true));
        for _ in 0..14 {
            assert_eq!(collapse_repeat(line), (None, false), "a repeat was written");
        }
        // Something else ends the run and carries the tally.
        let (note, write) = collapse_repeat("С‡С‚Рѕ-С‚Рѕ РґСЂСѓРіРѕРµ");
        assert!(write);
        assert_eq!(note.as_deref(), Some("в†‘ С‚Рѕ Р¶Рµ СЃР°РјРѕРµ РµС‰С‘ 14 СЂР°Р·"));
        // вЂ¦and the next line after that has nothing to report.
        assert_eq!(collapse_repeat("С‚СЂРµС‚СЊСЏ"), (None, true));

        // And a run that never ends is never silent: the tally is written every
        // `REPEAT_RUN`, so a problem repeating for an hour is still visible.
        // One test and not two: `collapse_repeat` keeps one run per process,
        // and two tests would race for it.
        let line = "Р±РµСЃРєРѕРЅРµС‡РЅРѕРµ";
        assert_eq!(collapse_repeat(line), (None, true));
        for _ in 0..(REPEAT_RUN - 1) {
            assert_eq!(collapse_repeat(line), (None, false));
        }
        let (note, write) = collapse_repeat(line);
        assert!(!write, "the repeat itself is still not written");
        assert_eq!(note.as_deref(), Some("в†‘ С‚Рѕ Р¶Рµ СЃР°РјРѕРµ РµС‰С‘ 50 СЂР°Р·"));
        // The counter restarted, so the next line ends a fresh run.
        assert_eq!(collapse_repeat(line), (None, false));
        assert_eq!(
            collapse_repeat("РёРЅРѕРµ").0.as_deref(),
            Some("в†‘ С‚Рѕ Р¶Рµ СЃР°РјРѕРµ РµС‰С‘ 1 СЂР°Р·")
        );
    }

    #[test]
    fn the_listener_address_is_loopback() {
        let ip: Ipv4Addr = LISTEN_IP.parse().expect("valid address");
        assert!(ip.is_loopback());
        // Not .1: something else on the machine may already own it.
        assert_ne!(ip, Ipv4Addr::LOCALHOST);
    }

    static CACHE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock_cache() -> std::sync::MutexGuard<'static, ()> {
        CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_detected_interface_is_cached_and_droppable() {
        let _guard = lock_cache();
        invalidate_interface();
        assert!(EGRESS_CACHE.lock().unwrap().is_none());
        if let Ok(mut c) = EGRESS_CACHE.lock() {
            *c = Some((17, Instant::now(), EGRESS_TTL));
        }
        // Served from the cache: no detection runs, so no process is spawned.
        assert_eq!(isp_interface(), 17);
        invalidate_interface();
        assert!(EGRESS_CACHE.lock().unwrap().is_none());
    }

    /// An expired entry must not be served - that is what makes the short retry
    /// after a failed detection actually retry.
    #[test]
    fn an_expired_entry_is_not_served() {
        let _guard = lock_cache();
        invalidate_interface();
        if let Ok(mut c) = EGRESS_CACHE.lock() {
            // Learned long ago, and only ever good for a moment.
            *c = Some((17, Instant::now() - Duration::from_secs(60), EGRESS_RETRY));
        }
        let stale = EGRESS_CACHE
            .lock()
            .unwrap()
            .map(|(_, at, good_for)| at.elapsed() >= good_for);
        assert_eq!(stale, Some(true));
        invalidate_interface();
    }

    /// A failed detection has to be forgotten quickly: at logon it usually just
    /// means the network is not up yet.
    #[test]
    fn a_failed_detection_is_remembered_only_briefly() {
        assert!(EGRESS_RETRY < EGRESS_TTL);
        assert!(EGRESS_RETRY <= Duration::from_secs(60));
    }

    /// Relays a real query end to end through the running upstream, and prints
    /// the verdict for every routed name.
    ///
    /// Needs a live network and the VPN OFF: through a tunnel every provider
    /// sees a foreign client and substitutes nothing, so every verdict comes
    /// back `Passthrough` and the run says nothing about the providers.
    ///
    /// The verdict is the assertion that matters now. The old version only
    /// checked that bytes came back - which stayed true through the whole
    /// outage that motivated `resolvers`, because an unsubstituted answer is
    /// still a perfectly well-formed answer.
    #[test]
    #[ignore = "needs a live network, VPN off; run with --ignored"]
    fn relays_a_real_query() {
        let id: u16 = 0x4242;
        let mut substituted = Vec::new();

        for name in [
            "cloudcode-pa.googleapis.com",
            "daily-cloudcode-pa.googleapis.com",
            "generativelanguage.googleapis.com",
            "antigravity-unleash.goog",
        ] {
            let mut query = vec![];
            query.extend_from_slice(&id.to_be_bytes());
            query.extend_from_slice(&[0x01, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
            for label in name.split('.') {
                query.push(label.len() as u8);
                query.extend_from_slice(label.as_bytes());
            }
            query.extend_from_slice(&[0, 0x00, 0x01, 0x00, 0x01]);

            let (reply, provider, verdict) = relay(&query).expect("a provider answered");
            assert_eq!(&reply[0..2], &id.to_be_bytes(), "id must be echoed");
            assert_eq!(
                dns_client::question_name(&reply).as_deref(),
                Some(name),
                "the reply must answer the question we asked"
            );
            assert!(
                u16::from_be_bytes([reply[6], reply[7]]) > 0,
                "{} came back with no answer",
                name
            );
            println!(
                "{:<38} {:?} via {:<12} {:?}",
                name,
                verdict,
                provider,
                dns_client::answer_addrs(&reply)
            );
            if verdict == Verdict::Substituted {
                substituted.push(name);
            }
        }

        // Not an assertion on any single name: which names a provider proxies
        // is theirs to change, and this test exists to report that, not to fail
        // on it. But if nothing at all is substituted, either the VPN is up or
        // every provider has dropped the whole list - both worth failing on.
        assert!(
            !substituted.is_empty(),
            "no routed name is substituted by any provider - VPN up, or the \
             providers dropped every name"
        );
        println!("substituted: {:?}", substituted);
    }

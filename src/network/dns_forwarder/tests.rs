    use super::*;
    use std::net::Ipv4Addr;

    /// A relay that has stopped going round must be visible as not serving.
    ///
    /// Measured on a machine where a relay's log went silent for two hours
    /// while its process stayed alive and held the machine's ports: the
    /// machine had a relay, a record frozen at the moment it stalled, and no
    /// bypass at all. The heartbeat is what tells the two apart.
    /// The heartbeat says the loop stopped; the step says where. Without this
    /// the five-minute kill left no clue which probe had hung.
    #[test]
    fn every_warm_step_has_a_name() {
        for i in 0..WARM_STEPS.len() {
            let saved = WARM_STEP.load(std::sync::atomic::Ordering::Relaxed);
            warm_step(i as u8);
            let name = warm_step_name();
            warm_step(saved);
            assert_ne!(name, "неизвестно", "step {i} has no name");
            assert!(!name.is_empty(), "step {i} has an empty name");
        }
    }

    #[test]
    fn a_silent_warm_loop_reads_as_a_stalled_relay() {
        let window = Duration::from_secs(300);
        // A second past the window: a stall.
        assert!(
            heartbeat_is_stale(100, 401, window),
            "a heartbeat older than the window is a stall"
        );
        // Exactly the window: still inside it.
        assert!(
            !heartbeat_is_stale(100, 400, window),
            "a relay inside its window is still serving"
        );
        // Ten minutes, with a ten-minute window: not yet.
        assert!(
            !heartbeat_is_stale(100, 700, Duration::from_secs(600)),
            "a relay inside its window is still serving"
        );
        // Never gone round: not a stall - a relay that has just started is
        // given its chance before it is called dead.
        assert!(
            !heartbeat_is_stale(0, 400, window),
            "a relay that has not gone round yet is not a stall"
        );
    }

    /// The loopback door broke account sign-in on the field machine. A queries
    /// for gate hosts must fall through to the substituted real address again —
    /// the v1.5.9 path that worked — instead of being answered with 127.65.71.x.
    #[test]
    fn a_gate_hosts_a_question_is_relayed_not_answered_by_the_door() {
        let q = dns_client::build_query("daily-cloudcode-pa.googleapis.com", 9);
        assert!(
            own_answer(&q, "daily-cloudcode-pa.googleapis.com").is_none(),
            "A queries must use the substituted address, not the loopback door"
        );
    }

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
        assert_eq!(human_minutes(Duration::from_secs(600)), "10 мин");
        assert_eq!(human_minutes(Duration::from_secs(3600)), "1 ч");
        assert_eq!(human_minutes(Duration::from_secs(6 * 3600)), "6 ч");
        assert_eq!(human_minutes(Duration::from_secs(90 * 60)), "90 мин");
    }

    /// Everything the version check does hangs off this: a relay that predates
    /// versioning leaves no file, and it must read as older than this build
    /// rather than as "no relay" or as a parse error nobody handles.
    #[test]
    fn an_unreadable_version_is_the_oldest_one() {
        assert_eq!(parse_version(None), 0);
        assert_eq!(parse_version(Some("")), 0);
        assert_eq!(parse_version(Some("не число")), 0);
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
        let line = "напрямую X: имя не разрешается";
        // The first is written, with nothing held back before it.
        assert_eq!(collapse_repeat(line), (None, true));
        for _ in 0..14 {
            assert_eq!(collapse_repeat(line), (None, false), "a repeat was written");
        }
        // Something else ends the run and carries the tally.
        let (note, write) = collapse_repeat("что-то другое");
        assert!(write);
        assert_eq!(note.as_deref(), Some("↑ то же самое ещё 14 раз"));
        // …and the next line after that has nothing to report.
        assert_eq!(collapse_repeat("третья"), (None, true));

        // And a run that never ends is never silent: the tally is written every
        // `REPEAT_RUN`, so a problem repeating for an hour is still visible.
        // One test and not two: `collapse_repeat` keeps one run per process,
        // and two tests would race for it.
        let line = "бесконечное";
        assert_eq!(collapse_repeat(line), (None, true));
        for _ in 0..(REPEAT_RUN - 1) {
            assert_eq!(collapse_repeat(line), (None, false));
        }
        let (note, write) = collapse_repeat(line);
        assert!(!write, "the repeat itself is still not written");
        assert_eq!(note.as_deref(), Some("↑ то же самое ещё 50 раз"));
        // The counter restarted, so the next line ends a fresh run.
        assert_eq!(collapse_repeat(line), (None, false));
        assert_eq!(
            collapse_repeat("иное").0.as_deref(),
            Some("↑ то же самое ещё 1 раз")
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

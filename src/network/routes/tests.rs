    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn sample(n: u64) -> Option<Sample> {
        Some(Sample {
            latency: ms(n),
            at: Instant::now(),
        })
    }

    /// The tests that change the table's evidence share one process-wide table,
    /// and `set_context` wipes it for everyone - so they take turns.
    static STATEFUL: Mutex<()> = Mutex::new(());

    fn turn() -> std::sync::MutexGuard<'static, ()> {
        STATEFUL.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn blank() -> Snapshot {
        Snapshot {
            samples: [None; N],
            penalised: [None; N],
            ok_at: [None; N],
            bad_at: [None; N],
            refused_at: [None; N],
            ok_count: [0; N],
            bad_count: [0; N],
            stumbled: [None; N],
            silent: [None; N],
            leader: None,
        }
    }

    #[test]
    fn unmeasured_routes_take_the_default_order() {
        let order = order_with(&blank(), true, |_| true);
        assert_eq!(
            order,
            vec![Kind::Own, Kind::Relay, Kind::Exits, Kind::Direct, Kind::Vpn]
        );
    }

    #[test]
    fn without_a_dns_layer_direct_goes_last_whatever_it_measured() {
        let mut s = blank();
        s.samples[Kind::Direct.index()] = sample(50);
        s.samples[Kind::Exits.index()] = sample(400);
        let order = order_with(&s, false, |k| k != Kind::Own && k != Kind::Vpn);
        assert_eq!(order, vec![Kind::Exits, Kind::Relay, Kind::Direct]);
    }

    #[test]
    fn measured_routes_go_fastest_first_and_unmeasured_after_them() {
        let mut s = blank();
        s.samples[Kind::Relay.index()] = sample(1300);
        s.samples[Kind::Exits.index()] = sample(450);
        let order = order_with(&s, true, |k| k != Kind::Own && k != Kind::Vpn);
        assert_eq!(order, vec![Kind::Exits, Kind::Relay, Kind::Direct]);
    }

    #[test]
    fn the_leader_keeps_its_place_unless_beaten_by_the_margin() {
        let mut s = blank();
        s.samples[Kind::Direct.index()] = sample(300);
        s.samples[Kind::Exits.index()] = sample(270);
        s.leader = Some(Kind::Direct);
        let usable = |k: Kind| k != Kind::Own && k != Kind::Vpn;
        assert_eq!(order_with(&s, true, usable)[0], Kind::Direct);
        s.samples[Kind::Exits.index()] = sample(180);
        assert_eq!(order_with(&s, true, usable)[0], Kind::Exits);
    }

    #[test]
    fn a_penalised_route_is_last_even_when_fastest() {
        let mut s = blank();
        s.samples[Kind::Direct.index()] = sample(100);
        s.samples[Kind::Relay.index()] = sample(1300);
        s.penalised[Kind::Direct.index()] = Some(Instant::now() + ms(60_000));
        s.leader = Some(Kind::Direct);
        let usable = |k: Kind| k != Kind::Own && k != Kind::Vpn;
        assert_eq!(
            order_with(&s, true, usable),
            vec![Kind::Relay, Kind::Exits, Kind::Direct]
        );
        s.penalised[Kind::Direct.index()] = Some(Instant::now() - ms(1));
        s.leader = None;
        assert_eq!(order_with(&s, true, usable)[0], Kind::Direct);
    }

    /// The whole point of proof: the fastest route is not the one that works.
    /// A slower route that carried a model answer goes ahead of a faster one
    /// that never has.
    #[test]
    fn a_route_that_carried_an_answer_beats_a_faster_one_that_did_not() {
        let mut s = blank();
        s.samples[Kind::Direct.index()] = sample(120);
        s.samples[Kind::Exits.index()] = sample(900);
        s.ok_at[Kind::Exits.index()] = Some(Instant::now());
        s.leader = Some(Kind::Direct);
        let usable = |k: Kind| k != Kind::Own && k != Kind::Vpn;
        assert_eq!(order_with(&s, true, usable)[0], Kind::Exits);
        // A refusal after the answer takes the proof away again.
        s.bad_at[Kind::Exits.index()] = Some(Instant::now() + ms(1));
        assert_eq!(order_with(&s, true, usable)[0], Kind::Direct);
    }

    /// A route that could not even open for a live connection steps aside for a
    /// minute, so the next connections do not each wait out its budget.
    #[test]
    fn a_route_that_failed_to_open_steps_aside() {
        let mut s = blank();
        s.samples[Kind::Direct.index()] = sample(100);
        s.samples[Kind::Exits.index()] = sample(500);
        s.stumbled[Kind::Direct.index()] = Some(Instant::now() + ms(60_000));
        let order = order_with(&s, true, |k| k != Kind::Own && k != Kind::Vpn);
        assert_eq!(order, vec![Kind::Exits, Kind::Relay, Kind::Direct]);
        s.stumbled[Kind::Direct.index()] = Some(Instant::now() - ms(1));
        assert_eq!(
            order_with(&s, true, |k| k != Kind::Own && k != Kind::Vpn)[0],
            Kind::Direct
        );
    }

    /// The field case behind `SILENT_STEPS`: a built-in exit that answered once
    /// and then went dead kept the table on itself, and the DNS route was never
    /// tried until the user switched the exits off.
    #[test]
    fn a_proven_route_that_went_silent_gives_way_to_the_dns_route() {
        let mut s = blank();
        s.samples[Kind::Exits.index()] = sample(400);
        s.samples[Kind::Direct.index()] = sample(600);
        s.ok_at[Kind::Exits.index()] = Some(Instant::now() - ms(600_000));
        s.leader = Some(Kind::Exits);
        let usable = |k: Kind| k == Kind::Exits || k == Kind::Direct;
        assert_eq!(order_with(&s, true, usable)[0], Kind::Exits);
        s.silent[Kind::Exits.index()] = Some(Instant::now() + ms(120_000));
        assert_eq!(
            order_with(&s, true, usable),
            vec![Kind::Direct, Kind::Exits]
        );
    }

    #[test]
    fn only_a_tunnel_the_client_spoke_into_and_heard_nothing_from_is_silent() {
        assert!(is_silent(0, 517), "ClientHello out, nothing back");
        assert!(is_silent(7, 517), "a TLS alert is not a server flight");
        assert!(!is_silent(0, 0), "never used by the client");
        assert!(!is_silent(4_800, 517), "a handshake came back");
    }

    #[test]
    fn silence_benches_once_per_episode_and_only_an_answer_lifts_it() {
        let _turn = turn();
        set_context(0x6666);
        assert_eq!(silence(Kind::Relay, false), Some(SILENT_STEPS[0]));
        // The client's other connections dying on the same route: one event.
        assert_eq!(silence(Kind::Relay, false), None);
        // A probe that passes does not let it out early.
        record(Kind::Relay, ms(300));
        assert!(snapshot().silent[Kind::Relay.index()].is_some());
        assert_eq!(
            order(|k| k == Kind::Relay || k == Kind::Exits)[0],
            Kind::Exits
        );
        // A model answer does, and protects it from the next silent tunnel.
        credit(Kind::Relay);
        assert!(snapshot().silent[Kind::Relay.index()].is_none());
        assert_eq!(silence(Kind::Relay, false), None);
        // An answer streaming through another of its tunnels protects it too.
        set_context(0x6667);
        assert_eq!(silence(Kind::Relay, true), None);
        // Escalates to the second step and stays there.
        if let Ok(mut t) = TABLE.lock() {
            t.silent_streak[Kind::Relay.index()] = 5;
        }
        assert_eq!(silence(Kind::Relay, false), Some(SILENT_STEPS[1]));
    }

    #[test]
    fn with_every_route_benched_the_one_benched_longest_ago_goes_first() {
        let mut s = blank();
        let now = Instant::now();
        s.penalised[Kind::Direct.index()] = Some(now + ms(600_000));
        s.penalised[Kind::Exits.index()] = Some(now + ms(60_000));
        s.penalised[Kind::Relay.index()] = Some(now + ms(3_600_000));
        let order = order_with(&s, true, |k| k != Kind::Own && k != Kind::Vpn);
        assert_eq!(order, vec![Kind::Exits, Kind::Direct, Kind::Relay]);
    }

    #[test]
    fn the_users_own_proxy_is_first_whenever_it_is_usable() {
        let mut s = blank();
        s.samples[Kind::Own.index()] = sample(900);
        s.samples[Kind::Direct.index()] = sample(100);
        s.ok_at[Kind::Direct.index()] = Some(Instant::now());
        s.leader = Some(Kind::Direct);
        assert_eq!(order_with(&s, true, |_| true)[0], Kind::Own);
        assert_eq!(order_with(&s, true, |k| k != Kind::Own)[0], Kind::Direct);
    }

    #[test]
    fn a_benched_own_proxy_gives_way_and_comes_back() {
        let now = Instant::now();
        let usable = |k: Kind| k != Kind::Vpn;
        for bench in [
            |s: &mut Snapshot, until| s.penalised[Kind::Own.index()] = Some(until),
            |s: &mut Snapshot, until| s.stumbled[Kind::Own.index()] = Some(until),
            |s: &mut Snapshot, until| s.silent[Kind::Own.index()] = Some(until),
        ] {
            let mut s = blank();
            bench(&mut s, now + ms(600_000));
            let order = order_with(&s, true, usable);
            assert_eq!(
                order,
                vec![Kind::Relay, Kind::Exits, Kind::Direct, Kind::Own]
            );

            // The bench over, first again.
            let mut s = blank();
            bench(&mut s, now - ms(1));
            assert_eq!(order_with(&s, true, usable)[0], Kind::Own);
        }
    }

    #[test]
    fn a_benched_own_proxy_sorts_among_the_benched_by_when_it_ends() {
        let now = Instant::now();
        let mut s = blank();
        s.penalised[Kind::Own.index()] = Some(now + ms(600_000));
        s.penalised[Kind::Relay.index()] = Some(now + ms(60_000));
        let order = order_with(&s, true, |k| k != Kind::Vpn);
        assert_eq!(
            order,
            vec![Kind::Exits, Kind::Direct, Kind::Relay, Kind::Own]
        );
        // An unusable Own is not offered at all, benched or not.
        let order = order_with(&s, true, |k| k != Kind::Vpn && k != Kind::Own);
        assert!(!order.contains(&Kind::Own));
    }

    #[test]
    fn unusable_routes_are_not_offered_at_all() {
        let order = order_with(&blank(), true, |k| k == Kind::Relay);
        assert_eq!(order, vec![Kind::Relay]);
        assert!(order_with(&blank(), true, |_| false).is_empty());
    }

    /// `usable` may ask the table questions of its own. This deadlocked once:
    /// `order` held the lock while `is_penalised` tried to take it, and the
    /// first live connection through the proxy never got its 200 (G30).
    #[test]
    fn ordering_may_consult_the_table_from_inside_usable() {
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = done.clone();
        std::thread::spawn(move || {
            let _ = order(|k| !is_penalised(k) && latency(k).is_none() || is_proven(k) || true);
            refresh_leader(|k| !is_penalised(k));
            let _ = rows(|k| !is_penalised(k));
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(
                Instant::now() < deadline,
                "order() deadlocked on its own table"
            );
            std::thread::sleep(ms(10));
        }
    }

    /// The static half, in one test because it shares process-wide state:
    /// timings blend and expire, a refusal benches for a growing time, an
    /// answer lifts the bench and proves the route, and a new network forgets
    /// both.
    #[test]
    fn the_table_learns_and_forgets() {
        let _turn = turn();
        record_failure(Kind::Relay);
        assert_eq!(latency(Kind::Relay), None);
        record(Kind::Relay, ms(1000));
        record(Kind::Relay, ms(500));
        assert_eq!(latency(Kind::Relay), Some(ms(750)), "blended");
        record_failure(Kind::Relay);
        assert_eq!(latency(Kind::Relay), None);

        set_context(0x1111);
        assert_eq!(blame(Kind::Relay), Blamed::Benched(PENALTY_STEPS[0]));
        assert!(is_penalised(Kind::Relay));
        // Already benched: the client's retries on the old connection are the
        // same refusal, not a new one.
        assert_eq!(blame(Kind::Relay), Blamed::AlreadyBenched);
        credit(Kind::Relay);
        assert!(!is_penalised(Kind::Relay), "an answer lifts the bench");
        assert!(is_proven(Kind::Relay));
        // Refused straight after an answer: the route is left alone entirely -
        // its tier included, which is what ├В┬л├Р┬╜├Р┬╡ ├СтАЪ├СтВм├Р┬╛├Р┬╜├С╞Т├СтАЪ├В┬╗ has to mean if the next
        // connection is not to be handed to somebody else (G76).
        assert!(matches!(blame(Kind::Relay), Blamed::Proven(_)));
        assert!(!is_penalised(Kind::Relay), "a proven route is not benched");
        assert!(is_proven(Kind::Relay), "and it keeps its place");
        assert!(
            rows(|k| k == Kind::Relay)
                .iter()
                .any(|r| r.label == Kind::Relay.label() && r.refused_ago.is_some()),
            "but the refusal is still on the record the window shows"
        );
        // Once the proof has aged out, the same refusal benches - at the first
        // step, because the answer reset the streak.
        age_out_answer(Kind::Relay);
        assert_eq!(blame(Kind::Relay), Blamed::Benched(PENALTY_STEPS[0]));
        // A new network forgets it all.
        assert!(set_context(0x2222));
        assert!(!is_penalised(Kind::Relay));
        assert!(!set_context(0x2222), "the same network is not a change");
    }

    #[test]
    fn repeated_refusals_bench_for_longer_each_time() {
        let _turn = turn();
        // Uses Vpn so it cannot collide with the test above on the statics.
        set_context(0x3333);
        for step in [0usize, 1, 2, 3, 3] {
            assert_eq!(
                blame(Kind::Vpn),
                Blamed::Benched(PENALTY_STEPS[step]),
                "step {step}"
            );
            if let Ok(mut t) = TABLE.lock() {
                t.penalised[Kind::Vpn.index()] = Some(Instant::now() - ms(1));
            }
        }
        credit(Kind::Vpn);
    }

    /// Pushes `kind`'s last answer out of `PROOF_PROTECTS_FOR` without waiting
    /// ninety seconds for it.
    fn age_out_answer(kind: Kind) {
        if let Ok(mut t) = TABLE.lock() {
            t.ok_at[kind.index()] = Instant::now().checked_sub(PROOF_PROTECTS_FOR + ms(1));
        }
    }

    /// The field case this exists for: a route answers, is refused two seconds
    /// later, and must keep both its place in the table and its open tunnels.
    #[test]
    fn an_answer_just_now_keeps_a_refusal_from_benching_the_route() {
        let _turn = turn();
        set_context(0x4444);
        credit(Kind::Own);
        // Ten refusals in a row while the answers keep coming change nothing:
        // no bench, and no streak to escalate once the protection does lapse.
        for _ in 0..10 {
            assert!(matches!(blame(Kind::Own), Blamed::Proven(_)));
            credit(Kind::Own);
        }
        assert!(!is_penalised(Kind::Own));
        age_out_answer(Kind::Own);
        assert_eq!(
            blame(Kind::Own),
            Blamed::Benched(PENALTY_STEPS[0]),
            "the first step, because every answer reset the streak"
        );
    }

    /// A failed probe is not the same as an unmeasured route: it has to keep the
    /// route out of the running, or the next connection goes straight back to
    /// something that just refused to answer one.
    #[test]
    fn a_failed_probe_keeps_the_route_back_where_a_lost_measurement_did_not() {
        let _turn = turn();
        set_context(0x5555);
        record(Kind::Direct, ms(300));
        record_failure(Kind::Direct);
        assert_eq!(latency(Kind::Direct), None);
        let after_loss = order(|_| true);
        probe_failed(Kind::Direct);
        let after_probe = order(|_| true);
        assert!(
            after_probe.last() == Some(&Kind::Direct) && after_loss.last() != Some(&Kind::Direct),
            "{after_loss:?} then {after_probe:?}"
        );
        assert!(PROBE_STUMBLE_FOR > STUMBLE_FOR, "a probe runs every 2 min");
    }

    /// Attribution by open tunnel, and the tunnel of a refused route closed from
    /// outside the thread that pumps it.
    #[test]
    fn a_refusal_is_pinned_on_the_tunnel_that_was_open_and_that_tunnel_is_closed() {
        let _turn = turn();
        use std::io::Read;
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bound");
        let addr = listener.local_addr().expect("addr");
        let mut far = TcpStream::connect(addr).expect("connected");
        let (near, _) = listener.accept().expect("accepted");

        // Exits carries a tunnel, on a thread of its own, the way `serve` does.
        let (tx, rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let _gate = begin_gate_connection(&near, "daily-cloudcode-pa.googleapis.com");
            note_used(Kind::Exits);
            tx.send(()).ok();
            // Stands in for the splice: returns when the socket is shut down.
            let mut buf = [0u8; 1];
            let mut near = near;
            let _ = near.read(&mut buf);
        });
        rx.recv().expect("opened");
        assert_eq!(attribute(Instant::now(), None), Some(Kind::Exits));
        assert_eq!(
            attribute(Instant::now(), Some("daily-cloudcode-pa.googleapis.com")),
            Some(Kind::Exits)
        );
        // An event about the other host is not this tunnel's.
        assert_eq!(
            attribute(Instant::now(), Some("cloudcode-pa.googleapis.com")),
            None
        );
        assert!(open_counts()[Kind::Exits.index()] >= 1);

        set_context(0x4444);
        blame(Kind::Exits);
        // The far end sees its connection go.
        far.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let mut buf = [0u8; 1];
        let got = far.read(&mut buf);
        assert!(matches!(got, Ok(0) | Err(_)), "the tunnel was not closed");
        far.shutdown(std::net::Shutdown::Both).ok();
        drop(far);
        server.join().expect("server thread");
        credit(Kind::Exits);
    }

    /// D32: while a tunnel that carried an answer is open, a refusal pinned on
    /// that route does nothing at all - no bench, and above all no cut. This is
    /// the field symptom as a test: a long answer streaming while something
    /// else on the machine keeps walking into the gate.
    #[test]
    fn an_answer_still_streaming_keeps_its_route_and_its_socket() {
        let _turn = turn();
        use std::io::{ErrorKind, Read};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bound");
        let addr = listener.local_addr().expect("addr");
        let mut far = TcpStream::connect(addr).expect("connected");
        let (near, _) = listener.accept().expect("accepted");

        let (tx, rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let _gate = begin_gate_connection(&near, "daily-cloudcode-pa.googleapis.com");
            note_used(Kind::Exits);
            tx.send(serving_activity()).ok();
            // Stands in for the splice: returns when the socket ends.
            let mut buf = [0u8; 1];
            let mut near = near;
            let _ = near.read(&mut buf);
            done_tx.send(()).ok();
        });
        let activity = rx
            .recv()
            .expect("opened")
            .expect("a gate tunnel has a stamp");

        set_context(0x5555);
        let (kind, id) = attribute_tunnel(Instant::now(), None).expect("attributed");
        assert_eq!(kind, Kind::Exits);
        credit_tunnel(id);
        // The answer hands the client bytes, the way a stream does.
        activity.to_client(8 * 1024);

        match blame(Kind::Exits) {
            Blamed::Streaming { to_client, .. } => assert_eq!(to_client, 8 * 1024),
            other => panic!("a streaming answer was not recognised: {other:?}"),
        }
        assert!(
            !is_penalised(Kind::Exits),
            "a route carrying an answer must not be benched"
        );
        // And the socket is still there: this is the half that cut answers in two.
        far.set_read_timeout(Some(Duration::from_millis(250))).ok();
        let mut buf = [0u8; 1];
        let got = far.read(&mut buf);
        assert!(
            matches!(&got, Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)),
            "the live answer's socket was cut: {got:?}"
        );

        // The client ends the exchange. Nothing is in flight any more, so the
        // next refusal benches the route exactly as it did before (D26).
        far.shutdown(std::net::Shutdown::Both).ok();
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the tunnel ended");
        server.join().expect("server thread");
        assert!(
            live_answer(Kind::Exits).is_none(),
            "a closed tunnel protects nothing"
        );
        assert!(matches!(blame(Kind::Exits), Blamed::Benched(_)));
        assert!(is_penalised(Kind::Exits));
    }

    /// G76, the flap three field reports (2026-09-21) show: a refusal the table
    /// declines to hold against a route must not cost it its place either.
    ///
    /// It did. `blame` stamped the refusal whatever it decided, `proven` reads
    /// that stamp, and the route dropped out of the proven tier the moment it
    /// was refused - so the leader swapped to whatever measured fastest, swapped
    /// back on the next answer, and did it again fifteen seconds later. Half the
    /// client's new connections went to a road nothing had proved, while the
    /// road that was answering sat second.
    #[test]
    fn a_protected_refusal_does_not_cost_the_route_its_place() {
        let _turn = turn();
        set_context(0x7601);
        let usable = |k: Kind| matches!(k, Kind::Exits | Kind::Relay);
        credit(Kind::Exits);
        record(Kind::Relay, ms(50));
        assert_eq!(order(usable).first(), Some(&Kind::Exits));

        // A refusal inside `PROOF_PROTECTS_FOR`: not evidence (D31).
        assert!(matches!(blame(Kind::Exits), Blamed::Proven(_)));
        assert!(
            is_proven(Kind::Exits),
            "a refusal that was not held against the route must not unprove it"
        );
        assert_eq!(
            order(usable).first(),
            Some(&Kind::Exits),
            "the route that answered must still be the one offered first"
        );

        // The user is still told it happened - that half was never the bug.
        let row = rows(usable)
            .into_iter()
            .find(|r| r.label == Kind::Exits.label())
            .expect("a row for the route");
        assert!(row.refused_ago.is_some(), "the report must still show it");
        assert_eq!((row.answers, row.refusals), (1, 1));
    }

    /// And the other way round, so the fix cannot quietly turn every refusal
    /// into a free one: once nothing protects the route, the refusal is
    /// evidence, the bench lands and the proof goes.
    #[test]
    fn an_unprotected_refusal_still_takes_the_proof_away() {
        let _turn = turn();
        set_context(0x7602);
        let usable = |k: Kind| matches!(k, Kind::Exits | Kind::Relay);
        credit(Kind::Exits);
        age_out_answer(Kind::Exits);
        assert!(matches!(blame(Kind::Exits), Blamed::Benched(_)));
        assert!(!is_proven(Kind::Exits));
        record(Kind::Relay, ms(50));
        assert_eq!(order(usable).first(), Some(&Kind::Relay));
    }

    /// The narrow cut: when one connection on a route is streaming an answer
    /// and the refusal arrived on a *different* one, that other one is closed
    /// and the streaming one is not. Anything else leaves the client retrying
    /// over a pooled connection that is being refused (D26) or cuts the answer
    /// in half (D32) - the two halves of the same field report.
    #[test]
    fn only_the_refused_connection_is_cut_beside_a_live_answer() {
        let _turn = turn();
        use std::io::{ErrorKind, Read};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bound");
        let addr = listener.local_addr().expect("addr");

        // Two gate tunnels on one route, the way a client's pool makes them.
        let open_one = |host: &'static str| {
            let far = TcpStream::connect(addr).expect("connected");
            let (near, _) = listener.accept().expect("accepted");
            let (tx, rx) = std::sync::mpsc::channel();
            let thread = std::thread::spawn(move || {
                let _gate = begin_gate_connection(&near, host);
                note_used(Kind::Exits);
                tx.send(attribute_tunnel(Instant::now(), Some(host))).ok();
                let mut buf = [0u8; 1];
                let mut near = near;
                let _ = near.read(&mut buf);
            });
            let (_, id) = rx.recv().expect("opened").expect("attributed");
            (far, id, thread)
        };
        let (mut answering, answering_id, t1) = open_one("daily-cloudcode-pa.googleapis.com");
        let (mut refused, refused_id, t2) = open_one("cloudcode-pa.googleapis.com");
        assert_ne!(answering_id, refused_id);

        set_context(0x7603);
        credit(Kind::Exits);
        credit_tunnel(answering_id);

        // The refusal came in on the sibling.
        assert!(matches!(blame(Kind::Exits), Blamed::Streaming { .. }));
        assert!(cut_tunnel(refused_id), "the refused connection must be cut");
        assert!(
            !cut_tunnel(answering_id),
            "the connection carrying the answer must never be cut"
        );

        refused.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let mut buf = [0u8; 1];
        assert!(
            matches!(refused.read(&mut buf), Ok(0) | Err(_)),
            "the refused connection is still open"
        );
        answering
            .set_read_timeout(Some(Duration::from_millis(250)))
            .ok();
        let got = answering.read(&mut buf);
        assert!(
            matches!(&got, Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)),
            "the answer's own connection was cut: {got:?}"
        );

        // `tunnel_shape` is what the log line reads to tell the two apart.
        assert!(tunnel_shape(answering_id)
            .expect("a shape")
            .answered_ago
            .is_some());
        assert!(tunnel_shape(refused_id)
            .expect("a shape")
            .answered_ago
            .is_none());

        refused.shutdown(std::net::Shutdown::Both).ok();
        answering.shutdown(std::net::Shutdown::Both).ok();
        drop(refused);
        drop(answering);
        t1.join().expect("first tunnel");
        t2.join().expect("second tunnel");
    }

    /// Both directions count as activity: a long chat is a 100 KB upload before
    /// it is an answer (field: `116225 B out` against `4253 B to client`).
    #[test]
    fn a_tunnels_activity_counts_both_directions() {
        let a = Activity::default();
        assert!(a.idle().is_none(), "nothing has moved yet");
        assert_eq!(a.bytes(), (0, 0));
        a.to_upstream(116_225);
        assert_eq!(a.bytes(), (0, 116_225));
        assert!(a.idle().is_some_and(|d| d < Duration::from_secs(1)));
        a.to_client(4_253);
        assert_eq!(a.bytes(), (4_253, 116_225));
    }

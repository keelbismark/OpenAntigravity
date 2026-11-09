    use super::*;

    /// The rotation must not cost an address its turn: whatever the start, the
    /// walk is still every index exactly once. This is the half that makes the
    /// fall-through safe, and it is the half a "just start somewhere random"
    /// implementation gets wrong.
    #[test]
    fn walk_visits_every_address_once_from_any_start() {
        for n in 1..=5usize {
            for start in 0..(n * 3) {
                let mut seen: Vec<usize> = walk(n, start).collect();
                assert_eq!(seen.len(), n, "n={n} start={start}: wrong length");
                seen.sort_unstable();
                seen.dedup();
                assert_eq!(seen.len(), n, "n={n} start={start}: an index repeated");
                assert_eq!(
                    *seen.last().unwrap(),
                    n - 1,
                    "n={n} start={start}: out of range"
                );
            }
        }
    }

    /// Consecutive calls start one further along вЂ” that is the whole point, and
    /// it is what splits the load across a service's peer machines instead of
    /// pinning every query to whichever one happens to be first in the list.
    #[test]
    fn walk_advances_by_one_per_call() {
        assert_eq!(walk(2, 0).next(), Some(0));
        assert_eq!(walk(2, 1).next(), Some(1));
        assert_eq!(walk(2, 2).next(), Some(0));
        assert_eq!(walk(3, 7).collect::<Vec<_>>(), vec![1, 2, 0]);
    }

    /// `NEXT_ADDR` is a process-lifetime counter, so it wraps eventually. The
    /// `wrapping_add` is why that is a non-event; without it this panics in a
    /// debug build, months after release, on a machine nobody can reproduce.
    #[test]
    fn walk_survives_counter_wraparound() {
        assert_eq!(
            walk(2, usize::MAX).collect::<Vec<_>>(),
            vec![usize::MAX % 2, 0]
        );
        assert_eq!(walk(3, usize::MAX - 1).count(), 3);
    }

    #[test]
    fn base64url_matches_rfc_vectors() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
        assert_eq!(base64url(b"fooba"), "Zm9vYmE");
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64url_is_url_safe() {
        // 0xfb 0xff produces '+' and '/' in standard base64; here it must not.
        let s = base64url(&[0xfb, 0xff, 0xfe]);
        assert!(
            !s.contains('+') && !s.contains('/') && !s.contains('='),
            "{}",
            s
        );
    }

    #[test]
    fn hpack_int_uses_one_octet_below_the_prefix_maximum() {
        let mut v = Vec::new();
        hpack_int(&mut v, 1, 6, 0x40);
        assert_eq!(v, vec![0x41]);
        let mut v = Vec::new();
        hpack_int(&mut v, 10, 7, 0x00);
        assert_eq!(v, vec![10]);
    }

    /// RFC 7541 В§C.1.1-C.1.2: 1337 with a 5-bit prefix is 31, 154, 10.
    #[test]
    fn hpack_int_continues_past_the_prefix() {
        let mut v = Vec::new();
        hpack_int(&mut v, 1337, 5, 0x00);
        assert_eq!(v, vec![31, 154, 10]);
    }

    /// The bug this guards: a query with EDNS options encodes to a path longer
    /// than a single-octet HPACK length can express, and a truncated length is a
    /// corrupt request rather than a slow one.
    #[test]
    fn a_long_path_still_encodes_its_length() {
        let long = "/dns-query?dns=".to_string() + &"A".repeat(400);
        let h = request_headers("dns.example", &long);
        // Length is 415 = 127 + 288 -> prefix octet then two continuation octets.
        let idx = h.windows(3).position(|w| w == [127, 160, 2]).map(|i| i);
        assert!(
            idx.is_some(),
            "РґР»РёРЅР° РїСѓС‚Рё Р·Р°РєРѕРґРёСЂРѕРІР°РЅР° РЅРµРІРµСЂРЅРѕ: {:?}",
            &h[..24]
        );
        assert!(h.ends_with(b"application/dns-message"));
    }

    #[test]
    fn request_headers_start_with_the_indexed_method_and_scheme() {
        let h = request_headers("dns.example", "/dns-query?dns=AAA");
        assert_eq!(h[0], 0x82, ":method GET");
        assert_eq!(h[1], 0x87, ":scheme https");
        assert_eq!(h[2], 0x01, ":authority");
    }

    /// Neither literal may carry the incremental-indexing bit: on a kept
    /// connection each unique `?dns=` would otherwise be inserted into the
    /// server's dynamic table. 0x01 / 0x04 are static-table names 1 and 4 with
    /// the 4-bit "without indexing" prefix (RFC 7541 В§6.2.2).
    #[test]
    fn nothing_in_the_request_is_indexed() {
        let host = "dns.example";
        let h = request_headers(host, "/dns-query?dns=AAA");
        let path_at = 2 + 1 + 1 + host.len();
        assert_eq!(h[2], 0x01, ":authority, literal without indexing");
        assert_eq!(h[path_at], 0x04, ":path, literal without indexing");
        for at in [2, path_at] {
            assert_ne!(h[at] & 0xc0, 0x40, "РѕРєС‚РµС‚ {}: incremental indexing", at);
        }
    }

    /// Type 0x8 on stream 0, four payload octets, and the increment that takes
    /// the connection window from 65 535 to exactly 2^31-1.
    #[test]
    fn window_update_raises_the_connection_window_to_the_maximum() {
        let f = window_update(0, CONN_WINDOW_INCREMENT);
        assert_eq!(
            f,
            vec![
                0,
                0,
                4,
                FRAME_WINDOW_UPDATE,
                0,
                0,
                0,
                0,
                0,
                0x7f,
                0xff,
                0x00,
                0x00
            ]
        );
        assert_eq!(65_535u64 + CONN_WINDOW_INCREMENT as u64, (1u64 << 31) - 1);
        // The reserved bit stays clear whatever is asked for.
        assert_eq!(window_update(3, u32::MAX)[9], 0x7f);
    }

    #[test]
    fn frame_header_is_nine_octets_big_endian() {
        let f = frame(FRAME_DATA, FLAG_END_STREAM, 1, b"xy");
        assert_eq!(&f[..3], &[0, 0, 2]);
        assert_eq!(f[3], FRAME_DATA);
        assert_eq!(f[4], FLAG_END_STREAM);
        assert_eq!(&f[5..9], &[0, 0, 0, 1]);
        assert_eq!(&f[9..], b"xy");
    }

    /// A scripted server: SETTINGS, then HEADERS, then DATA split across two
    /// frames with END_STREAM on the last.
    struct Scripted {
        to_read: Vec<u8>,
        pos: usize,
        written: Vec<u8>,
        /// Bytes per `read`: small by default, to split frames across reads.
        max_read: usize,
    }

    impl ReadWrite for Scripted {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = (self.to_read.len() - self.pos)
                .min(buf.len())
                .min(self.max_read);
            buf[..n].copy_from_slice(&self.to_read[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
        fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
            self.written.extend_from_slice(buf);
            Ok(())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn scripted(script: Vec<u8>) -> Scripted {
        Scripted {
            to_read: script,
            pos: 0,
            written: Vec::new(),
            max_read: 7,
        }
    }

    fn read_stream(s: &mut Scripted, buf: &mut Vec<u8>, stream: u32) -> Result<Answer, Fail> {
        read_reply(
            s,
            buf,
            stream,
            "127.0.0.1".parse().unwrap(),
            Instant::now() + Duration::from_secs(5),
        )
    }

    fn run(script: Vec<u8>) -> (Result<Vec<u8>, String>, Vec<u8>) {
        let mut s = scripted(script);
        let out = read_stream(&mut s, &mut Vec::new(), 1)
            .map(|a| a.body)
            .map_err(Fail::into_message);
        (out, s.written)
    }

    #[test]
    fn reassembles_data_split_across_frames_and_reads() {
        let mut script = Vec::new();
        script.extend_from_slice(&frame(FRAME_SETTINGS, 0, 0, &[]));
        script.extend_from_slice(&frame(FRAME_HEADERS, FLAG_END_HEADERS, 1, &[0x88]));
        script.extend_from_slice(&frame(FRAME_DATA, 0, 1, b"hello "));
        script.extend_from_slice(&frame(FRAME_DATA, FLAG_END_STREAM, 1, b"world"));
        let (out, written) = run(script);
        assert_eq!(out.unwrap(), b"hello world");
        // The server's SETTINGS must have been acknowledged.
        assert_eq!(written, frame(FRAME_SETTINGS, FLAG_ACK, 0, &[]));
    }

    #[test]
    fn a_ping_is_acknowledged_with_its_own_payload() {
        let mut script = Vec::new();
        script.extend_from_slice(&frame(FRAME_PING, 0, 0, &[1, 2, 3, 4, 5, 6, 7, 8]));
        script.extend_from_slice(&frame(FRAME_DATA, FLAG_END_STREAM, 1, b"ok"));
        let (out, written) = run(script);
        assert_eq!(out.unwrap(), b"ok");
        assert!(written.ends_with(&[1, 2, 3, 4, 5, 6, 7, 8]));
    }

    #[test]
    fn goaway_and_rst_stream_are_reported_not_waited_out() {
        let (out, _) = run(frame(FRAME_GOAWAY, 0, 0, &[0, 0, 0, 0, 0, 0, 0, 11]));
        assert!(out.unwrap_err().contains("GOAWAY"));

        let (out, _) = run(frame(FRAME_RST_STREAM, 0, 1, &[0, 0, 0, 8]));
        assert!(out.unwrap_err().contains("СЃР±СЂРѕС€РµРЅ"));
    }

    #[test]
    fn a_headers_only_response_is_an_error_not_an_empty_answer() {
        let script = frame(
            FRAME_HEADERS,
            FLAG_END_HEADERS | FLAG_END_STREAM,
            1,
            &[0x88],
        );
        let (out, _) = run(script);
        assert!(out.unwrap_err().contains("Р±РµР· С‚РµР»Р°"));
    }

    /// A length field larger than any DNS reply must be refused outright rather
    /// than used to size an allocation.
    #[test]
    fn an_absurd_frame_length_is_refused() {
        let (out, _) = run(vec![0xff, 0xff, 0xff, FRAME_DATA, 0, 0, 0, 0, 1]);
        assert!(out.unwrap_err().contains("РЅРµ РѕС‚РІРµС‚ DNS"));
    }

    /// The second query on a connection is stream 3: DATA for any other stream
    /// is not its answer, and whatever arrives after its END_STREAM stays in
    /// the buffer, whole, for the next query.
    #[test]
    fn a_later_stream_skips_other_streams_and_keeps_the_tail() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&frame(FRAME_DATA, FLAG_END_STREAM, 1, b"stale"));
        buf.extend_from_slice(&frame(FRAME_HEADERS, FLAG_END_HEADERS, 3, &[0x88]));
        buf.extend_from_slice(&frame(FRAME_DATA, FLAG_END_STREAM, 3, b"mine"));
        let tail = frame(FRAME_PING, 0, 0, &[9, 9, 9, 9, 9, 9, 9, 9]);
        buf.extend_from_slice(&tail);
        // A partial frame behind it: its first bytes must survive as well.
        buf.extend_from_slice(&frame(FRAME_WINDOW_UPDATE, 0, 0, &[0, 0, 1, 0])[..5]);
        let expected_tail = buf[buf.len() - tail.len() - 5..].to_vec();

        // Nothing to read from the socket: all of it arrived in one piece.
        let mut s = scripted(Vec::new());
        let answer = read_stream(&mut s, &mut buf, 3).unwrap();
        assert_eq!(answer.body, b"mine");
        assert!(!answer.last);
        assert_eq!(buf, expected_tail, "С…РІРѕСЃС‚ РїРѕСЃР»Рµ END_STREAM РїРѕС‚РµСЂСЏРЅ");
        assert!(s.written.is_empty(), "the PING belongs to the next call");
    }

    /// Two answers in one read: the first call must stop at its own
    /// END_STREAM and the second must find its answer already buffered -
    /// without reading anything more, since nothing more is coming.
    #[test]
    fn two_answers_in_one_piece_are_read_by_two_calls() {
        let mut script = Vec::new();
        script.extend_from_slice(&frame(FRAME_HEADERS, FLAG_END_HEADERS, 1, &[0x88]));
        script.extend_from_slice(&frame(FRAME_DATA, FLAG_END_STREAM, 1, b"first"));
        script.extend_from_slice(&frame(FRAME_HEADERS, FLAG_END_HEADERS, 3, &[0x88]));
        script.extend_from_slice(&frame(FRAME_DATA, 0, 3, b"sec"));
        script.extend_from_slice(&frame(FRAME_DATA, FLAG_END_STREAM, 3, b"ond"));
        let mut s = scripted(script);
        s.max_read = usize::MAX;
        let mut buf = Vec::new();
        assert_eq!(read_stream(&mut s, &mut buf, 1).unwrap().body, b"first");
        assert_eq!(s.pos, s.to_read.len(), "one read took everything");
        assert_eq!(read_stream(&mut s, &mut buf, 3).unwrap().body, b"second");
        assert!(buf.is_empty());
    }

    /// Frames left from the previous query are handled before anything is
    /// read: a PING that arrived behind the last answer is acknowledged.
    #[test]
    fn frames_left_by_the_last_query_are_handled_first() {
        let mut buf = frame(FRAME_PING, 0, 0, &[1, 2, 3, 4, 5, 6, 7, 8]);
        let mut s = scripted(frame(FRAME_DATA, FLAG_END_STREAM, 5, b"ok"));
        assert_eq!(read_stream(&mut s, &mut buf, 5).unwrap().body, b"ok");
        assert_eq!(
            s.written,
            frame(FRAME_PING, FLAG_ACK, 0, &[1, 2, 3, 4, 5, 6, 7, 8])
        );
    }

    /// GOAWAY that still covers our stream: the answer is coming, so read it -
    /// and mark the connection finished.
    #[test]
    fn goaway_covering_our_stream_still_delivers_the_answer() {
        let mut script = frame(FRAME_GOAWAY, 0, 0, &[0, 0, 0, 3, 0, 0, 0, 0]);
        script.extend_from_slice(&frame(FRAME_DATA, FLAG_END_STREAM, 3, b"ok"));
        let answer = read_stream(&mut scripted(script), &mut Vec::new(), 3).unwrap();
        assert_eq!(answer.body, b"ok");
        assert!(answer.last, "РїРѕСЃР»Рµ GOAWAY СЃРѕРµРґРёРЅРµРЅРёРµ РЅРµ РІРѕР·РІСЂР°С‰Р°СЋС‚ РІ РїСѓР»");
    }

    /// GOAWAY below our stream: the server says it will not process it. That is
    /// `Gone` - worth one retry on a fresh connection - not `Broken`.
    #[test]
    fn goaway_short_of_our_stream_is_gone() {
        let script = frame(FRAME_GOAWAY, 0, 0, &[0, 0, 0, 5, 0, 0, 0, 0]);
        let out = read_stream(&mut scripted(script), &mut Vec::new(), 7);
        assert!(
            matches!(out, Err(Fail::Gone(ref m)) if m.contains("GOAWAY")),
            "{:?}",
            out
        );
    }

    /// EOF before any frame of our answer: the kept connection had been closed
    /// under us, so `Gone`. EOF halfway through the answer: `Broken` - and not
    /// the truncated bytes passed off as an answer.
    #[test]
    fn an_early_eof_is_gone_and_a_late_one_is_broken() {
        let out = read_stream(&mut scripted(Vec::new()), &mut Vec::new(), 3);
        assert!(matches!(out, Err(Fail::Gone(_))), "{:?}", out);

        let script = frame(FRAME_DATA, 0, 3, b"half");
        let out = read_stream(&mut scripted(script), &mut Vec::new(), 3);
        assert!(matches!(out, Err(Fail::Broken(_))), "{:?}", out);
    }

    /// A malformed RST_STREAM (payload shorter than its 4-octet code) is an
    /// error, not an index panic.
    #[test]
    fn a_short_rst_stream_is_an_error_not_a_panic() {
        let (out, _) = run(frame(FRAME_RST_STREAM, 0, 1, &[0]));
        assert!(out.unwrap_err().contains("СЃР±СЂРѕС€РµРЅ"));
    }

    #[test]
    fn a_connection_is_taken_only_while_fresh_and_under_its_request_limit() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        assert!(takeable(t0, 0, t0));
        assert!(takeable(t0, 1, t0 + s(19)));
        assert!(!takeable(t0, 1, t0 + IDLE_LIMIT), "РїСЂРѕСЃС‚РѕР№ 20 СЃ вЂ” СѓР¶Рµ РЅРµС‚");
        assert!(!takeable(t0, 1, t0 + s(29)));
        assert!(takeable(t0, MAX_REQUESTS - 1, t0 + s(1)));
        assert!(!takeable(t0, MAX_REQUESTS, t0 + s(1)));
        // A `now` earlier than `last_used` is not idle time.
        assert!(takeable(t0 + s(5), 1, t0));
    }

    #[test]
    fn a_connection_goes_back_only_after_a_clean_answer() {
        let answer = |last: bool| -> Result<Answer, Fail> {
            Ok(Answer {
                body: b"x".to_vec(),
                last,
            })
        };
        assert!(keep_after(&answer(false), 1));
        assert!(!keep_after(&Err(Fail::Gone("x".into())), 1));
        assert!(!keep_after(&Err(Fail::Broken("x".into())), 1));
        assert!(!keep_after(&answer(true), 1), "GOAWAY");
        assert!(keep_after(&answer(false), MAX_REQUESTS - 1));
        assert!(
            !keep_after(&answer(false), MAX_REQUESTS),
            "РёСЃС‡РµСЂРїР°РЅ Р»РёРјРёС‚ Р·Р°РїСЂРѕСЃРѕРІ"
        );
    }

    /// One retry, only for a kept connection that turned out to be gone. A
    /// fresh connection failing is the address failing - `query` walks on.
    #[test]
    fn only_a_gone_kept_connection_is_retried() {
        assert!(retry_on_fresh(true, &Fail::Gone("x".into())));
        assert!(!retry_on_fresh(true, &Fail::Broken("x".into())));
        assert!(!retry_on_fresh(false, &Fail::Gone("x".into())));
        assert!(!retry_on_fresh(false, &Fail::Broken("x".into())));
    }

    /// The whole point, end to end: the hand-written h2 client gets a real answer
    /// out of a real DoH server, and that answer is a **substitution**.
    ///
    /// The second half is the assertion that matters. "It answered" is what let
    /// an upstream change go unnoticed for a release (see `resolvers`), so this
    /// compares against 8.8.8.8 and fails if the reply merely carries genuine
    /// Google - which is what a passthrough looks like and parses perfectly.
    #[test]
    #[ignore = "needs a live network, VPN off; run with --ignored"]
    fn reaches_a_real_doh_server_and_gets_a_substitution() {
        use crate::dns_client;
        let endpoint = &crate::resolvers::DNS_AI;

        for name in [
            "cloudcode-pa.googleapis.com",
            "daily-cloudcode-pa.googleapis.com",
        ] {
            let q = dns_client::build_query(name, 0x7A7A);
            let reply = query(endpoint, &q, Duration::from_secs(8))
                .unwrap_or_else(|e| panic!("{}: {}", name, e));

            assert_eq!(&reply[0..2], &q[0..2], "id must come back");
            let got = dns_client::answer_addrs(&reply);
            assert!(!got.is_empty(), "{}: РїСѓСЃС‚РѕР№ РѕС‚РІРµС‚", name);

            let reference = dns_client::resolve_a_via(name, "8.8.8.8".parse().unwrap(), 0)
                .expect("reference resolver");
            let ref16: Vec<[u8; 2]> = reference
                .iter()
                .map(|a| [a.octets()[0], a.octets()[1]])
                .collect();
            let passthrough = got.iter().any(|a| match a {
                IpAddr::V4(v) => ref16.contains(&[v.octets()[0], v.octets()[1]]),
                IpAddr::V6(_) => false,
            });
            assert!(
                !passthrough,
                "{}: РѕС‚РІРµС‚ {:?} Р»РµР¶РёС‚ РІ С‚РѕР№ Р¶Рµ /16, С‡С‚Рѕ Рё СЌС‚Р°Р»РѕРЅ {:?} - СЌС‚Рѕ passthrough, \
                 Р° РЅРµ РїРѕРґРјРµРЅР°",
                name, got, reference
            );
            println!("{} -> {:?} (СЌС‚Р°Р»РѕРЅ {:?})", name, got, reference);
        }
    }

    /// Live: **every** node, not just whichever answers first - each address is
    /// asked for both gate names on its own connection, and each answer has to
    /// be a substitution (P14: "the addresses too"). One dead or passthrough node
    /// in the walk costs a share of all queries a timeout or a wrong answer, and
    /// `reaches_a_real_doh_server_and_gets_a_substitution` would not notice it
    /// as long as another node answered first (G43).
    ///
    ///     cargo test every_node_substitutes_both_gate_names -- --ignored --nocapture
    #[test]
    #[ignore = "needs a live network, VPN off; run with --ignored"]
    fn every_node_substitutes_both_gate_names() {
        use crate::dns_client;
        let ep = &crate::resolvers::DNS_AI;
        for addr in ep.addrs {
            let ip: IpAddr = addr.parse().expect("an address");
            for name in [
                "cloudcode-pa.googleapis.com",
                "daily-cloudcode-pa.googleapis.com",
            ] {
                let q = dns_client::build_query(name, 0x4E4E);
                let started = Instant::now();
                let reply = query_one(&Pool::new(), ep, ip, &q, Duration::from_secs(8))
                    .unwrap_or_else(|e| panic!("{} {}: {}", addr, name, e));
                let got = dns_client::answer_addrs(&reply);
                let reference = dns_client::resolve_a_via(name, "8.8.8.8".parse().unwrap(), 0)
                    .expect("reference resolver");
                let ref16: Vec<[u8; 2]> = reference
                    .iter()
                    .map(|a| [a.octets()[0], a.octets()[1]])
                    .collect();
                let passthrough = got.iter().any(|a| match a {
                    IpAddr::V4(v) => ref16.contains(&[v.octets()[0], v.octets()[1]]),
                    IpAddr::V6(_) => false,
                });
                println!(
                    "{:<15} {:<34} {:>4} РјСЃ  {:?}",
                    addr,
                    name,
                    started.elapsed().as_millis(),
                    got
                );
                assert!(!got.is_empty(), "{} {}: РїСѓСЃС‚РѕР№ РѕС‚РІРµС‚", addr, name);
                assert!(
                    !passthrough,
                    "{} {}: {:?} вЂ” РїРѕРґР»РёРЅРЅС‹Р№ Google, СѓР·РµР» РЅРµ РїРѕРґРјРµРЅСЏРµС‚",
                    addr, name, got
                );
            }
        }
    }

    /// Each address resumes the session **it** issued. The walk alternates the
    /// two nodes, and rustls hands out the newest ticket stored under the server
    /// name - so with one store per process every connection presented the
    /// *other* node's ticket, which that node cannot decrypt (ticket keys are
    /// per node, on purpose). The server side counted it as
    /// `tlsunknownticketkeys`, ~26/s per node, each one a full handshake.
    ///
    /// A, B, then A again is exactly that interleaving: with a shared store the
    /// second round is `Full`.
    ///
    /// A node that resumes nothing at all is left out, and said so: msk1
    /// (`94.232.43.149`, nginx) issues two tickets on every handshake and
    /// accepts none, measured 2026-09-18 - a server setting, not something a
    /// client store can fix. `node_resumes` asks each node on a store of its own.
    #[test]
    #[ignore = "needs a live network, VPN off; run with --ignored"]
    fn each_address_resumes_its_own_session() {
        use crate::dns_client;
        use std::sync::atomic::Ordering::Relaxed;
        let ep = &crate::resolvers::DNS_AI;
        let ips: Vec<IpAddr> = ep.addrs.iter().map(|a| a.parse().unwrap()).collect();
        assert!(ips.len() >= 2, "the point is two peer nodes");
        let resuming: Vec<IpAddr> = ips
            .iter()
            .copied()
            .filter(|&ip| node_resumes(ep, ip))
            .collect();
        for ip in ips.iter().filter(|ip| !resuming.contains(ip)) {
            println!("{}: СѓР·РµР» РЅРµ РїСЂРёРЅРёРјР°РµС‚ РґР°Р¶Рµ СЃРІРѕРё Р±РёР»РµС‚С‹ вЂ” РїСЂРѕРїСѓС‰РµРЅ", ip);
        }
        assert!(!resuming.is_empty(), "РЅРё РѕРґРёРЅ СѓР·РµР» РЅРµ РІРѕР·РѕР±РЅРѕРІР»СЏРµС‚ СЃРµСЃСЃРёРё");
        let q = dns_client::build_query("cloudcode-pa.googleapis.com", 0x5151);

        // One exchange per node, in walk order, each on a connection that was
        // a FULL handshake: then every store holds tickets its node has just
        // issued, whatever an earlier test left in it or took out. Reading the
        // reply is what takes the tickets in.
        let pool = Pool::new();
        for &ip in &ips {
            for _ in 0..10 {
                let before = pool.resumed.load(Relaxed);
                query_one(&pool, ep, ip, &q, Duration::from_secs(8))
                    .unwrap_or_else(|e| panic!("{}", e));
                pool.clear();
                if pool.resumed.load(Relaxed) == before {
                    break;
                }
            }
        }
        for &ip in &resuming {
            let (_, conn) = handshake(ep, ip, Instant::now() + Duration::from_secs(8))
                .unwrap_or_else(|e| panic!("{}", e));
            assert_eq!(
                conn.handshake_kind(),
                Some(HandshakeKind::Resumed),
                "{}: РїРѕР»РЅРѕРµ СЂСѓРєРѕРїРѕР¶Р°С‚РёРµ вЂ” РїСЂРµРґСЉСЏРІР»РµРЅ С‡СѓР¶РѕР№ Р±РёР»РµС‚",
                ip
            );
        }
    }

    /// Whether `ip` resumes a session it issued at all, asked on a config of
    /// the test's own so the per-address stores under test are not touched:
    /// the first connection takes in the node's tickets, the second presents
    /// one. `handshake_kind` alone cannot tell "this node resumes nothing"
    /// from "we offered it another node's ticket" - both are `Full`.
    ///
    /// Each connection carries one whole query before it is closed, as every
    /// real one does. Closing as soon as the tickets were in - no exchange at
    /// all - made msk3 and spb1 refuse them on the next connection (observed
    /// 2026-09-18; the mechanism on their side is unmeasured), which read as
    /// "no node resumes".
    fn node_resumes(ep: &Endpoint, ip: IpAddr) -> bool {
        let mut cfg = base_tls_config().clone();
        // 16 like `tls_config`, for the reason given there: 4 stores nothing.
        cfg.resumption = Resumption::in_memory_sessions(16);
        let cfg = Arc::new(cfg);
        let q = crate::dns_client::build_query("cloudcode-pa.googleapis.com", 0x5252);
        let path = format!("{}?dns={}", ep.path, base64url(&q));
        let mut kind = None;
        for _ in 0..2 {
            let deadline = Instant::now() + Duration::from_secs(5);
            let sock =
                TcpStream::connect_timeout(&SocketAddr::new(ip, 443), Duration::from_secs(5))
                    .unwrap_or_else(|e| panic!("{}: {}", ip, e));
            let mut sock = Bounded { sock, deadline };
            let name = ServerName::try_from(ep.host.to_string()).unwrap();
            let mut conn = ClientConnection::new(cfg.clone(), name).unwrap();
            while conn.is_handshaking() {
                conn.complete_io(&mut sock)
                    .unwrap_or_else(|e| panic!("{}: {}", ip, e));
            }
            kind = conn.handshake_kind();

            let mut out = PREFACE.to_vec();
            out.extend_from_slice(&frame(FRAME_SETTINGS, 0, 0, &[]));
            out.extend_from_slice(&frame(
                FRAME_HEADERS,
                FLAG_END_HEADERS | FLAG_END_STREAM,
                1,
                &request_headers(ep.host, &path),
            ));
            let mut tls = rustls::Stream::new(&mut conn, &mut sock);
            std::io::Write::write_all(&mut tls, &out).unwrap();
            std::io::Write::flush(&mut tls).unwrap();
            read_reply(&mut tls, &mut Vec::new(), 1, ip, deadline)
                .unwrap_or_else(|f| panic!("{}", f.into_message()));
            conn.send_close_notify();
            let _ = conn.write_tls(&mut sock);
        }
        kind == Some(HandshakeKind::Resumed)
    }
    /// A well-formed answer to `q`: same id, a response, NOERROR, and at
    /// least one address. "Valid DNS" and not just "bytes came back" - an
    /// error page or a frame from the wrong stream fails here.
    fn assert_answers(q: &[u8], reply: &[u8]) {
        assert!(reply.len() >= 12, "РєРѕСЂРѕС‚РєРёР№ РѕС‚РІРµС‚: {:?}", reply);
        assert_eq!(&reply[0..2], &q[0..2], "id must come back");
        assert_ne!(reply[2] & 0x80, 0, "QR: СЌС‚Рѕ РЅРµ РѕС‚РІРµС‚");
        assert_eq!(reply[3] & 0x0f, 0, "RCODE {}", reply[3] & 0x0f);
        assert!(
            !crate::dns_client::answer_addrs(reply).is_empty(),
            "РЅРё РѕРґРЅРѕРіРѕ Р°РґСЂРµСЃР°"
        );
    }

    /// The point of the pool, measured: 50 queries in a row, walked across both
    /// nodes the way the relay walks them. Before the pool that was 50 TCP
    /// connections and 50 TLS handshakes.
    ///
    /// The assertion is on what the CLIENT decides: past the first connection
    /// to each node, a new one is opened only to replace one that was dropped -
    /// closed by the server under us, retired after a GOAWAY, or given up after
    /// an error. The server does close them - measured 2026-09-18, msk3 and spb1,
    /// a clean close_notify with no GOAWAY after anywhere from 2 to 69 queries,
    /// about one query in twenty - so a fixed "at most 4" holds on some runs
    /// and not others, and says nothing either way about this code.
    ///
    /// Then the pool is emptied, and new connections to each node have to
    /// resume the session that node issued. The service issues two tickets
    /// per full handshake and none per resumption (measured: F R R F R R ...),
    /// and a TLS 1.3 ticket is single-use, so a node's store can be empty at
    /// that moment; of any two consecutive new connections, though, one must
    /// resume. Two fulls in a row is the bug step 1 fixed.
    #[test]
    #[ignore = "needs a live network, VPN off; run with --ignored"]
    fn keeps_one_connection_per_address_and_resumes_after() {
        use crate::dns_client;
        use std::sync::atomic::Ordering::Relaxed;
        let ep = &crate::resolvers::DNS_AI;
        let pool = Pool::new();
        let names = [
            "cloudcode-pa.googleapis.com",
            "daily-cloudcode-pa.googleapis.com",
        ];

        for i in 0..50u16 {
            let q = dns_client::build_query(names[i as usize % 2], 0x1000 + i);
            let reply = query_via(&pool, ep, &q, Duration::from_secs(8))
                .unwrap_or_else(|e| panic!("Р·Р°РїСЂРѕСЃ {}: {}", i, e));
            assert_answers(&q, &reply);
        }
        let opened = pool.opened.load(Relaxed);
        let dropped = pool.dropped.load(Relaxed);
        let retried = pool.retried.load(Relaxed);
        println!(
            "50 Р·Р°РїСЂРѕСЃРѕРІ: {} СЃРѕРµРґРёРЅРµРЅРёР№; РІС‹Р±СЂРѕС€РµРЅРѕ РїРѕСЃР»Рµ Р·Р°РїСЂРѕСЃР° {}, РёР· РЅРёС… {} Р·Р°РєСЂС‹Р» СЃРµСЂРІРµСЂ",
            opened, dropped, retried
        );
        assert!(
            opened <= ep.addrs.len() + dropped,
            "{} СЃРѕРµРґРёРЅРµРЅРёР№ РЅР° {} Р°РґСЂРµСЃР° РїСЂРё {} РІС‹Р±СЂРѕС€РµРЅРЅС‹С… - РєР»РёРµРЅС‚ РѕС‚РєСЂС‹РІР°РµС‚ Р»РёС€РЅРёРµ",
            opened,
            ep.addrs.len(),
            dropped
        );

        for (i, addr) in ep.addrs.iter().enumerate() {
            let ip: IpAddr = addr.parse().unwrap();
            if !node_resumes(ep, ip) {
                println!("{}: СѓР·РµР» РЅРµ РїСЂРёРЅРёРјР°РµС‚ РґР°Р¶Рµ СЃРІРѕРё Р±РёР»РµС‚С‹ вЂ” РїСЂРѕРїСѓС‰РµРЅ", ip);
                continue;
            }
            let mut kinds = String::new();
            for round in 0..2u16 {
                pool.clear();
                let before = pool.resumed.load(Relaxed);
                let q = dns_client::build_query(names[0], 0x2000 + i as u16 * 8 + round);
                let reply = query_one(&pool, ep, ip, &q, Duration::from_secs(8))
                    .unwrap_or_else(|e| panic!("{}", e));
                assert_answers(&q, &reply);
                kinds.push(if pool.resumed.load(Relaxed) > before {
                    'R'
                } else {
                    'F'
                });
            }
            println!("{}: РїРѕСЃР»Рµ СЃР±СЂРѕСЃР° РїСѓР»Р° {}", ip, kinds);
            assert_ne!(kinds, "FF", "{}: РґРІР° РїРѕР»РЅС‹С… СЂСѓРєРѕРїРѕР¶Р°С‚РёСЏ РїРѕРґСЂСЏРґ", ip);
        }
        pool.clear();
    }

    /// Flow control, against the real server: one connection carries more
    /// than the 65 535 bytes of connection window it starts with. Without the
    /// WINDOW_UPDATE sent at open, the server stops sending DATA somewhere
    /// past that mark and the query waits out its budget.
    ///
    /// TXT with EDNS for two names whose answers are 4.5-5.4 KB (measured), so
    /// the mark is passed in ~14 queries: the server closes a connection about
    /// once in twenty queries, and one that is closed early just means the
    /// count starts again on the next.
    #[test]
    #[ignore = "needs a live network, VPN off; run with --ignored"]
    fn one_connection_carries_more_than_the_initial_window() {
        use crate::dns_client;
        use std::sync::atomic::Ordering::Relaxed;
        let ep = &crate::resolvers::DNS_AI;
        let ip: IpAddr = ep.addrs[0].parse().unwrap();
        let pool = Pool::new();

        let txt_query = |name: &str, id: u16| {
            let mut q = dns_client::build_query(name, id);
            let at = q.len() - 3;
            q[at] = 16; // TXT
            q[11] = 1; // one additional record: OPT
                       // OPT: root name, type 41, 4096-byte payload, no flags, no data.
            q.extend_from_slice(&[0, 0, 41, 0x10, 0x00, 0, 0, 0, 0, 0, 0]);
            q
        };

        let mut carried = 0usize;
        let mut on = 0usize;
        let mut sent = 0u16;
        while carried <= 70_000 {
            assert!(
                sent < 300,
                "Р·Р° 300 Р·Р°РїСЂРѕСЃРѕРІ РЅРё РѕРґРЅРѕ СЃРѕРµРґРёРЅРµРЅРёРµ РЅРµ РЅР°Р±СЂР°Р»Рѕ 70 РљР‘"
            );
            let name = ["microsoft.com", "amazon.com"][sent as usize % 2];
            let q = txt_query(name, 0x3000 + sent);
            let reply = query_one(&pool, ep, ip, &q, Duration::from_secs(8))
                .unwrap_or_else(|e| panic!("РїРѕСЃР»Рµ {} Р±Р°Р№С‚ РїРѕ СЃРѕРµРґРёРЅРµРЅРёСЋ: {}", carried, e));
            assert_eq!(&reply[0..2], &q[0..2]);
            let now_on = pool.opened.load(Relaxed);
            if now_on != on {
                on = now_on;
                carried = 0;
            }
            carried += reply.len();
            sent += 1;
        }
        println!(
            "{} Р±Р°Р№С‚ РїРѕ РѕРґРЅРѕРјСѓ СЃРѕРµРґРёРЅРµРЅРёСЋ; {} Р·Р°РїСЂРѕСЃРѕРІ, {} СЃРѕРµРґРёРЅРµРЅРёР№",
            carried,
            sent,
            pool.opened.load(Relaxed)
        );
        pool.clear();
    }

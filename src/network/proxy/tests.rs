    use super::*;

    /// Live: what the direct route would dial for each gate host - on Linux the
    /// provider pool's substituted address, never the system resolver's Google.
    ///
    ///     cargo test the_direct_route_dials_a_substituted_address -- --ignored --nocapture
    #[test]
    #[ignore = "needs a live network from a gated region; run with --ignored"]
    fn the_direct_route_dials_a_substituted_address() {
        for host in [
            "cloudcode-pa.googleapis.com",
            "daily-cloudcode-pa.googleapis.com",
        ] {
            match gate_direct_addrs(host, 443) {
                Ok((addrs, pin)) => println!("{host:<36} -> {addrs:?} (pin {pin})"),
                Err(why) => println!("{host:<36} -> refused: {why}"),
            }
        }
    }

    /// The listener check must answer about the socket, and a refused connection
    /// must read as "no listener".
    ///
    /// This is the gate in front of `HTTPS_PROXY` (G31, I53). The helper it
    /// replaces returned `true` when it could not parse its own URL, i.e. it
    /// reported a dead proxy as a live one - the exact direction of error that
    /// leaves a user-wide variable pointing at nothing.
    #[test]
    fn a_socket_nobody_holds_reads_as_no_listener() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral port");
        let addr = listener.local_addr().expect("addr");

        assert!(answers_at(addr), "a bound port must answer");
        assert!(
            wait_at(addr, Duration::from_secs(1)),
            "waiting on a bound port must return at once"
        );

        drop(listener);
        assert!(!answers_at(addr), "a closed port must not answer");
    }

    /// And it must keep looking for the whole budget rather than deciding on one
    /// probe: the port can be held for a few seconds by somebody's ephemeral
    /// socket while `bind_listener` retries it.
    #[test]
    fn waiting_for_a_listener_that_never_comes_costs_the_budget_and_says_no() {
        // Port 1, not an ephemeral one just released: the OS hands those out
        // again, and any other test in this process that binds a listener while
        // this one is waiting made it fail for a reason that has nothing to do
        // with what it tests. Nothing listens on tcpmux on Windows.
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 1));

        let budget = Duration::from_millis(600);
        let started = Instant::now();
        assert!(!wait_at(addr, budget));
        assert!(
            started.elapsed() >= budget,
            "gave up after {:?}, before the budget was spent",
            started.elapsed()
        );
    }

    /// A status line that lost its carriage returns is still valid Rust and
    /// still compiles - it just produces a response no HTTP client accepts.
    /// Written as raw byte values so no tool can normalise the test itself.
    #[test]
    fn status_lines_are_crlf_terminated() {
        for line in [RESP_ESTABLISHED, RESP_BAD_GATEWAY, RESP_NOT_ALLOWED] {
            assert_eq!(&line[line.len() - 4..], &[13u8, 10, 13, 10], "{:?}", line);
            assert!(!line[..line.len() - 4].contains(&10u8), "{:?}", line);
        }
    }

    /// A tunnel must not be closed by a budget that was only ever meant to bound
    /// the CONNECT handshake.
    ///
    /// The regression this pins: `upstream::open` left its ten-second reply budget
    /// on the socket it returned, `splice` handed that socket to `io::copy`, and a
    /// timeout reads as end-of-stream - so every tunnel through the user's own
    /// proxy or a built-in exit died at 10.3 s of silence. A pooling client
    /// reconnects on the failed reuse, which showed up as a long hang on
    /// "Authenticating" and 35 tunnels in 25 seconds in the log.
    ///
    /// Sixteen seconds of silence, comfortably past the old ten, then the tunnel
    /// is used - a live client would fail here, not at the handshake.
    ///
    ///     cargo test an_idle_tunnel_outlives_the_connect_budget -- --ignored --nocapture
    #[test]
    #[ignore = "holds a tunnel open for 16 s against a real route; needs a live network"]
    fn an_idle_tunnel_outlives_the_connect_budget() {
        use rustls::pki_types::ServerName;
        use rustls::ClientConnection;
        use std::net::TcpListener;

        const HOST: &str = "daily-cloudcode-pa.googleapis.com";
        const IDLE: Duration = Duration::from_secs(16);

        let listener = TcpListener::bind("127.0.0.1:0").expect("bound");
        let addr = listener.local_addr().expect("addr");
        thread::spawn(move || {
            let (sock, _) = listener.accept().expect("accepted");
            serve(sock, 0);
        });

        let mut client = TcpStream::connect(addr).expect("connected");
        client
            .write_all(
                format!("CONNECT {HOST}:443 HTTP/1.1\r\nHost: {HOST}:443\r\n\r\n").as_bytes(),
            )
            .expect("sent CONNECT");
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            assert_eq!(client.read(&mut byte).expect("reply"), 1, "proxy hung up");
            head.push(byte[0]);
        }
        assert!(String::from_utf8_lossy(&head).contains(" 200"));

        let name = ServerName::try_from(HOST).expect("name");
        let mut tls = ClientConnection::new(probe_config(), name).expect("tls");
        let mut stream = rustls::Stream::new(&mut tls, &mut client);
        // Complete the handshake before going quiet, so the silence is measured on
        // an established tunnel - which is the state a pooled connection sits in.
        stream.flush().ok();

        thread::sleep(IDLE);

        stream
            .write_all(
                format!(
                    "GET /v1internal:probe HTTP/1.1\r\nHost: {HOST}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .expect("the tunnel was closed under an idle client");
        let mut buf = [0u8; 64];
        let n = stream.read(&mut buf).expect("no answer after idling");
        assert!(
            buf[..n].starts_with(b"HTTP/"),
            "not an HTTP answer: {:?}",
            String::from_utf8_lossy(&buf[..n])
        );
        println!(
            "tunnel survived {} s idle and still carried a request",
            IDLE.as_secs()
        );
    }

    #[test]
    fn reads_the_connect_target() {
        assert_eq!(
            parse_connect(
                "CONNECT jetski-webchannel.googleapis.com:443 HTTP/1.1\r\nHost: x\r\n\r\n"
            ),
            Some(("jetski-webchannel.googleapis.com".to_string(), 443))
        );
        assert_eq!(
            parse_connect("connect example.com:8443 HTTP/1.1\r\n\r\n"),
            Some(("example.com".to_string(), 8443))
        );
        assert_eq!(
            parse_connect("CONNECT [::1]:443 HTTP/1.1\r\n\r\n"),
            Some(("::1".to_string(), 443))
        );
    }

    /// The bug the IDE actually hit. A client that opens a proxy socket and has
    /// not spoken yet must be closed in silence: answering it puts a status line
    /// where its CONNECT response belongs, and the client reports only "Proxy
    /// connection ended before receiving CONNECT response".
    #[test]
    fn a_socket_that_says_nothing_is_closed_without_an_answer() {
        use std::net::TcpListener;

        fn ask(send: Option<&'static [u8]>) -> Request {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let addr = listener.local_addr().unwrap();
            let writer = thread::spawn(move || {
                let mut c = TcpStream::connect(addr).unwrap();
                match send {
                    Some(bytes) => {
                        c.write_all(bytes).unwrap();
                        // Held open so the read cannot end on the socket closing
                        // instead of on the request being complete.
                        thread::sleep(Duration::from_millis(200));
                    }
                    None => drop(c),
                }
            });
            let (mut sock, _) = listener.accept().unwrap();
            sock.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let got = read_connect(&mut sock);
            writer.join().ok();
            got
        }

        assert_eq!(ask(None), Request::Gone);
        assert_eq!(
            ask(Some(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")),
            Request::Malformed
        );
        assert_eq!(
            ask(Some(
                b"CONNECT a.googleapis.com:443 HTTP/1.1\r\nHost: a\r\n\r\n"
            )),
            Request::Connect("a.googleapis.com".to_string(), 443)
        );
    }

    /// Anything that is not a CONNECT is refused rather than guessed at.
    #[test]
    fn a_non_connect_request_is_refused() {
        assert_eq!(parse_connect("GET / HTTP/1.1\r\n\r\n"), None);
        assert_eq!(parse_connect("CONNECT nohost HTTP/1.1\r\n\r\n"), None);
        assert_eq!(parse_connect("CONNECT :443 HTTP/1.1\r\n\r\n"), None);
        assert_eq!(parse_connect(""), None);
    }

    #[test]
    fn a_proxy_on_this_machine_or_the_lan_is_local_and_a_public_one_is_not() {
        for h in [
            "127.0.0.1",
            "localhost",
            "192.168.1.10",
            "10.0.0.2",
            "[::1]",
            "mybox",
        ] {
            assert!(is_local_host(h), "{h}");
        }
        for h in ["proxy.example.com", "203.0.113.5", "8.8.8.8"] {
            assert!(!is_local_host(h), "{h}");
        }
        // Not a gate host, or no loopback door: the name, always.
        assert_eq!(connect_target("127.0.0.1", "example.com"), "example.com");
        if !crate::loopback::active() {
            assert_eq!(
                connect_target("127.0.0.1", "daily-cloudcode-pa.googleapis.com"),
                "daily-cloudcode-pa.googleapis.com"
            );
        }
    }

    #[test]
    fn test_live_metrics_tracks_initial_state() {
        let m = live_metrics();
        assert_eq!(m.active_tunnels, 0);
    }

    #[test]
    fn the_proxy_url_is_loopback() {
        assert!(proxy_url().starts_with("http://127.0.0.1:"));
    }

    /// P26: a port another program listens on is left at once - no fifteen
    /// seconds of retries for a holder that will not leave - and the proxy
    /// takes the next one, which becomes the port everything names.
    #[test]
    fn a_held_port_moves_the_proxy_to_the_next_one_at_once() {
        let holder = TcpListener::bind("127.0.0.1:0").unwrap();
        let held = holder.local_addr().unwrap().port();
        let free = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let started = Instant::now();
        let got = bind_from(held, &[free]).expect("moved to the free port");
        assert_eq!(got.local_addr().unwrap().port(), free);
        assert_eq!(port(), free);
        assert!(proxy_url().ends_with(&format!(":{free}")));
        assert!(started.elapsed() < BIND_RETRY_WAIT, "retried a held port");
    }

    /// With nowhere to go the failure is reported, naming who holds the port.
    #[test]
    fn a_held_port_with_nowhere_to_go_says_who_holds_it() {
        let holder = TcpListener::bind("127.0.0.1:0").unwrap();
        let held = holder.local_addr().unwrap().port();
        let b = bind_from(held, &[]).err().expect("nothing to move to");
        assert_eq!(b.what, "proxy");
        if cfg!(windows) {
            assert_eq!(b.cause, "held");
        }
        assert!(b.addr.ends_with(&format!(":{held}")));
    }

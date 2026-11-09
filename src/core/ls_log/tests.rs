    use super::*;
    use std::io::Write;

    fn temp_log(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("open_antigravity_ls_log_tests");
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join(format!("{}-{}.log", name, std::process::id()));
        let _ = fs::remove_file(&path);
        path
    }

    fn append(path: &Path, text: &str) {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open");
        f.write_all(text.as_bytes()).expect("write");
    }

    const REFUSAL: &str = "E0901 15:09:24.614939 1 stream_handler.go:101] FAILED_PRECONDITION (code 400): User location is not supported for the API use.\n";
    const NOISE: &str = "I0901 15:09:24.639090 1 server.go:427] Setting GOMAXPROCS to 4\n";

    /// The two shapes the real logs are actually written in, copied off this
    /// machine. Neither starts with the glog header: the Desktop server prefixes
    /// every line, and the IDE wraps the prefixed line again.
    const DESKTOP_LINE: &str = "ERROR: logging before google.Init: E0901 15:09:24.614939       1 stream_handler.go:101] FAILED_PRECONDITION (code 400): User location is not supported for the API use.";
    const IDE_LINE: &str = "2026-09-12 03:57:46.735 [error] [LS Main stderr] ERROR: logging before google.Init: E0901 15:09:24.614939       1 stream_handler.go:101] FAILED_PRECONDITION (code 400): User location is not supported for the API use.";

    #[test]
    fn counts_the_refusal_case_insensitively_and_nothing_else() {
        assert_eq!(count_refusals(REFUSAL.as_bytes()), 1);
        assert_eq!(count_refusals(NOISE.as_bytes()), 0);
        assert_eq!(
            count_refusals(b"USER LOCATION IS NOT SUPPORTED x user location is not supported"),
            2
        );
        assert_eq!(
            count_refusals(b"FAILED_PRECONDITION (code 400): something else"),
            0,
            "the status alone is not the region gate"
        );
        assert_eq!(count_refusals(b""), 0);
    }

    fn refusals_in(path: &Path, from: Option<u64>) -> (u64, usize) {
        let (off, t) = scan(path, from, None, &mut None);
        (off, t.refusals)
    }

    #[test]
    fn history_is_not_news_but_what_is_appended_is() {
        let path = temp_log("append");
        append(&path, REFUSAL);
        append(&path, REFUSAL);
        // First sight: taken from the end, old refusals ignored.
        let (off, hits) = refusals_in(&path, None);
        assert_eq!(hits, 0);
        assert_eq!(off, fs::metadata(&path).unwrap().len());
        // Nothing appended: nothing found, offset unchanged.
        assert_eq!(refusals_in(&path, Some(off)), (off, 0));
        // Appended: found once, and only once.
        append(&path, NOISE);
        append(&path, REFUSAL);
        let (off2, hits) = refusals_in(&path, Some(off));
        assert_eq!(hits, 1);
        assert_eq!(refusals_in(&path, Some(off2)), (off2, 0));
        // A refusal split by a buffer flush: half in one pass, half in the next.
        let cut = REFUSAL.len() / 2;
        append(&path, &REFUSAL[..cut]);
        let (off3, hits) = refusals_in(&path, Some(off2));
        assert_eq!(hits, 0);
        append(&path, &REFUSAL[cut..]);
        let (off4, hits) = refusals_in(&path, Some(off3));
        assert_eq!(hits, 1, "the halves must be read together");
        assert_eq!(refusals_in(&path, Some(off4)), (off4, 0), "and not counted again");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_file_that_shrank_is_read_from_the_start() {
        let path = temp_log("shrink");
        for _ in 0..8 {
            append(&path, NOISE);
        }
        let (off, _) = refusals_in(&path, None);
        assert!(
            off > REFUSAL.len() as u64,
            "the rewrite below must shrink it"
        );
        // The app restarted and truncated its log; the new content is news.
        fs::write(&path, REFUSAL).expect("truncate");
        let (off2, hits) = refusals_in(&path, Some(off));
        assert_eq!(hits, 1);
        assert_eq!(off2, REFUSAL.len() as u64);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_changes_nothing() {
        let path = temp_log("missing");
        assert_eq!(refusals_in(&path, None), (0, 0));
        assert_eq!(refusals_in(&path, Some(42)), (42, 0));
    }

    /// The two real success lines, copied off this machine (Desktop wraps glog in
    /// its own prefix; the CLI writes bare glog). The refused turn in the IDE log
    /// of 2026-09-12 had no such line at all, and the calls that are logged on a
    /// refused session too must not count.
    #[test]
    fn a_model_answer_is_the_generation_call_with_a_response_id() {
        const DESKTOP_OK: &str = "ERROR: logging before google.Init: I0918 17:07:13.969105    1230 http_helpers.go:296] URL: https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse Trace: 0xbf323913e34d92f8 ResponseID: kUWtatHf";
        const CLI_OK: &str = "I0918 18:56:33.939879     359 http_helpers.go:299] URL: https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse Trace: 0x26e77a0ae05c274c ResponseID: MV-taof4GP2TvdIPieiCmQs";
        const LOAD: &str = "I0918 17:12:08.722720    2138 http_helpers.go:296] URL: https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist Trace: 0x65704d0ab55a0ef7";
        assert!(is_answer(DESKTOP_OK.as_bytes()));
        assert!(is_answer(CLI_OK.as_bytes()));
        assert!(!is_answer(LOAD.as_bytes()));
        assert!(!is_answer(REFUSAL.as_bytes()));
        // The stamp of a success line is placed like a refusal's.
        assert!(parse_stamp(DESKTOP_OK).is_some());
        assert!(parse_stamp(CLI_OK).is_some());

        let mut host = None;
        let t = tally(
            format!("{DESKTOP_OK}\n{LOAD}\n{REFUSAL}{CLI_OK}\n").as_bytes(),
            Some(clock(9, 18, 18, 57, 0)),
            &mut host,
        );
        assert_eq!(t.answers, 2);
        assert_eq!(t.refusals, 1);
        // Newest answer: the CLI line at 18:56:33, 27 s before the clock.
        assert_eq!(t.answered_ago, Some(Duration::from_secs(27)));
        // The refusal's stamp is 1 September, not today: unplaceable.
        assert_eq!(t.refused_ago, None);
        // Hosts: the answer names its own; the refusal takes the last URL line
        // before it, the `loadCodeAssist` one.
        assert_eq!(t.answered_host.as_deref(), Some("daily-cloudcode-pa.googleapis.com"));
        assert_eq!(t.refused_host.as_deref(), Some("daily-cloudcode-pa.googleapis.com"));
        assert_eq!(host.as_deref(), Some("daily-cloudcode-pa.googleapis.com"));
        // A URL line for some other host is not a gate host and changes nothing.
        assert_eq!(url_host(b"I0918 x] URL: https://oauth2.googleapis.com/token"), None);
        assert_eq!(
            url_host(b"I0918 x] URL: https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist Trace"),
            Some("cloudcode-pa.googleapis.com".to_string())
        );
    }

    fn clock(month: u16, day: u16, h: u32, m: u32, s: u32) -> crate::utils::LocalClock {
        crate::utils::LocalClock {
            month,
            day,
            second_of_day: h * 3600 + m * 60 + s,
        }
    }

    /// The window's reader, end to end: tail, stamp, age, count. Written with
    /// the machine's own clock so the stamps are the shape the language server
    /// would have written a moment ago.
    #[test]
    fn a_refusal_is_found_by_its_own_stamp_and_an_old_one_is_not() {
        let Some(now) = crate::utils::local_clock() else {
            return; // no local clock (non-Windows): the reader answers None by design
        };
        // Not within the first hour after midnight: the stamps below are built
        // by subtracting from the clock, and "yesterday" is a different test.
        if now.second_of_day < 3600 {
            return;
        }
        // Written the way the real Desktop log writes it, prefix and all, so
        // this exercises the search rather than a shape only a test produces.
        let stamp = |back: u32| {
            let sod = now.second_of_day - back;
            format!(
                "ERROR: logging before google.Init: E{:02}{:02} {:02}:{:02}:{:02}.123456       1 stream_handler.go:101] ",
                now.month,
                now.day,
                sod / 3600,
                (sod / 60) % 60,
                sod % 60
            )
        };
        let path = temp_log("recent");
        append(&path, &format!("{}Setting GOMAXPROCS to 4\n", stamp(30)));
        append(&path, &format!("{}FAILED_PRECONDITION (code 400): User location is not supported for the API use.\n", stamp(1500)));
        append(&path, &format!("{}FAILED_PRECONDITION (code 400): User location is not supported for the API use.\n", stamp(300)));
        append(&path, &format!("{}FAILED_PRECONDITION (code 400): User location is not supported for the API use.\n", stamp(120)));

        let files = [path.clone()];
        let seen = newest_refusal(&files, Duration::from_secs(600)).expect("two are in the window");
        assert_eq!(seen.count, 2, "the 25-minute-old one is outside it");
        assert!(
            (115..=135).contains(&seen.ago.as_secs()),
            "newest was two minutes ago, got {:?}",
            seen.ago
        );
        // A window narrower than the newest refusal finds nothing at all.
        assert_eq!(newest_refusal(&files, Duration::from_secs(60)), None);
        // And a log with nothing in it says so rather than guessing.
        let quiet = temp_log("quiet");
        append(&quiet, &format!("{}Setting GOMAXPROCS to 4\n", stamp(10)));
        assert_eq!(
            newest_refusal(std::slice::from_ref(&quiet), Duration::from_secs(600)),
            None
        );
        assert!(bytes_of(&files) > 0);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&quiet);
    }

    /// The window's one-pass reader must agree with the two single-question ones
    /// it replaced, window by window.
    #[test]
    fn one_pass_history_matches_the_single_questions() {
        let Some(now) = crate::utils::local_clock() else {
            return;
        };
        if now.second_of_day < 3600 {
            return;
        }
        let stamp = |back: u32, level: char| {
            let sod = now.second_of_day - back;
            format!(
                "{}{:02}{:02} {:02}:{:02}:{:02}.123456       1 x.go:1] ",
                level,
                now.month,
                now.day,
                sod / 3600,
                (sod / 60) % 60,
                sod % 60
            )
        };
        let path = temp_log("history");
        let refusal = "FAILED_PRECONDITION (code 400): User location is not supported for the API use.";
        let answer = "URL: https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse Trace: 0x1 ResponseID: abc";
        append(&path, &format!("{}{}
", stamp(1800, 'E'), refusal));
        append(&path, &format!("{}{}
", stamp(900, 'I'), answer));
        append(&path, &format!("{}{}
", stamp(120, 'E'), refusal));
        append(&path, &format!("{}{}
", stamp(60, 'E'), refusal));
        let files = [path.clone()];
        let h = history(&files, Duration::from_secs(600), Duration::from_secs(3600));
        assert_eq!(h.refused_recent, newest_refusal(&files, Duration::from_secs(600)));
        assert_eq!(h.refused, newest_refusal(&files, Duration::from_secs(3600)));
        assert_eq!(h.answered, newest_answer(&files, Duration::from_secs(3600)));
        assert_eq!(h.refused_recent.map(|s| s.count), Some(2));
        assert_eq!(h.refused.map(|s| s.count), Some(3));
        assert_eq!(h.answered.map(|s| s.count), Some(1));
        let _ = fs::remove_file(&path);
    }

    /// What the machine has right now. Reads, never asserts: the point is that
    /// the paths and the parse work against a real client log.
    ///
    ///     cargo test reads_what_the_client_actually_logged -- --ignored --nocapture
    #[test]
    #[ignore = "reads the real Antigravity logs on this machine; run with --ignored"]
    fn reads_what_the_client_actually_logged() {
        let logs = candidate_logs();
        println!("Р»РѕРіРѕРІ: {}, Р±Р°Р№С‚: {}", logs.len(), bytes_of(&logs));
        for path in &logs {
            println!("  {}", path.display());
        }
        println!(
            "РїРѕСЃР»РµРґРЅСЏСЏ region-400 Р·Р° СЃСѓС‚РєРё: {:?}",
            newest_refusal(&logs, Duration::from_secs(24 * 3600))
        );
    }

    /// The regression this parser was rewritten for: measured on the machine's
    /// own logs, the header is never at the start of the line. Anchored at byte
    /// 0 it matched neither real file, and the window would have reported
    /// silence while the user watched the error.
    #[test]
    fn the_header_is_found_under_both_wrappers_the_real_logs_use() {
        let expected = Some((9, 1, 15 * 3600 + 9 * 60 + 24));
        assert_eq!(parse_stamp(DESKTOP_LINE), expected);
        assert_eq!(parse_stamp(IDE_LINE), expected);
        // The IDE's own `2026-09-12 03:57:46.735` must not be mistaken for it:
        // what is read is the language server's stamp, inside the wrapper.
        assert_eq!(parse_stamp("2026-09-12 03:57:46.735 [info] [LS Main] Args"), None);
        // And a stamp-shaped run deep inside a message is out of reach.
        let deep = format!("{}E0102 03:04:05.000000 x", "y".repeat(HEADER_SEARCH));
        assert_eq!(parse_stamp(&deep), None);
    }

    #[test]
    fn a_glog_header_is_read_and_anything_else_refused() {
        assert_eq!(parse_stamp(REFUSAL), Some((9, 1, 15 * 3600 + 9 * 60 + 24)));
        assert_eq!(parse_stamp(NOISE), Some((9, 1, 15 * 3600 + 9 * 60 + 24)));
        // The tail cuts the first line anywhere, and half a header is not one.
        assert_eq!(parse_stamp("24.614939 1 stream_handler.go:101] blah"), None);
        assert_eq!(parse_stamp(""), None);
        assert_eq!(parse_stamp("E0901 15:09"), None);
        // Not a date and not a time.
        assert_eq!(parse_stamp("E1301 15:09:24.1 x"), None, "month 13");
        assert_eq!(parse_stamp("E0932 15:09:24.1 x"), None, "day 32");
        assert_eq!(parse_stamp("E0901 25:09:24.1 x"), None, "hour 25");
        // A Cyrillic first byte must not panic the slicing.
        assert_eq!(parse_stamp("Р­0901 15:09:24.614939 x"), None);
    }

    /// The header carries no year, so only today and yesterday can be placed -
    /// and "yesterday" is what makes a refusal at 23:58 still readable at 00:02.
    #[test]
    fn a_stamp_is_placed_against_the_local_clock() {
        let now = clock(9, 1, 15, 10, 0);
        assert_eq!(age_secs((9, 1, 15 * 3600 + 9 * 60), now), Some(60));
        assert_eq!(age_secs((9, 1, 15 * 3600 + 10 * 60), now), Some(0));
        // A stamp a second ahead of our own read of the clock is now.
        assert_eq!(age_secs((9, 1, 15 * 3600 + 10 * 60 + 2), now), Some(0));
        // Further ahead than that is not a line about now.
        assert_eq!(age_secs((9, 1, 15 * 3600 + 20 * 60), now), None);
        // Another day, and not the one before: unplaceable.
        assert_eq!(age_secs((8, 20, 0), now), None);
        // Just after midnight, looking back over it.
        let midnight = clock(9, 2, 0, 2, 0);
        assert_eq!(age_secs((9, 1, 23 * 3600 + 58 * 60), midnight), Some(240));
        // And over the end of a month.
        let first = clock(9, 1, 0, 1, 0);
        assert_eq!(age_secs((8, 31, 23 * 3600 + 59 * 60), first), Some(120));
        // Not the last day of August, so not the day before 1 September.
        assert_eq!(age_secs((8, 30, 23 * 3600 + 59 * 60), first), None);
        // February is allowed both its lengths.
        assert!(is_last_day(2, 28) && is_last_day(2, 29) && !is_last_day(2, 27));
    }

    #[test]
    fn the_newest_session_folder_wins() {
        let dir = std::env::temp_dir().join(format!("open_antigravity_sessions_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let old = dir.join("20260101T000000");
        let new = dir.join("20260901T191103");
        fs::create_dir_all(&old).unwrap();
        // A pause, so the two folders cannot share a timestamp; and the names
        // sort the same way, which is the tie-break either way.
        std::thread::sleep(std::time::Duration::from_millis(40));
        fs::create_dir_all(&new).unwrap();
        assert_eq!(newest_session(&dir), Some(new));
        let _ = fs::remove_dir_all(&dir);
    }

    const NOW: (u16, u16, u32, u32, u32) = (9, 25, 12, 0, 0);

    fn verify_in(text: &str) -> Option<Verification> {
        let (mo, d, h, mi, s) = NOW;
        newest_verification(text, clock(mo, d, h, mi, s), Duration::from_secs(3600))
    }

    /// Google's own shape: a pretty-printed body, `&` escaped as `\u0026`, in
    /// the IDE's framing with a blank line in the middle of it.
    #[test]
    fn a_verification_link_is_read_from_a_pretty_printed_body() {
        let log = concat!(
            "2026-09-25 11:58:00.100 [error] [LS Main stderr] ERROR: logging before google.Init: E0925 11:58:00.100000 1 http_helpers.go:88] URL: https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent Status: 403 Body: {\n",
            "  \"error\": {\n",
            "    \"code\": 403,\n",
            "    \"message\": \"Verify your account to continue.\",\n",
            "    \"status\": \"PERMISSION_DENIED\",\n",
            "\n",
            "    \"details\": [\n",
            "      {\n",
            "        \"@type\": \"type.googleapis.com/google.rpc.ErrorInfo\",\n",
            "        \"reason\": \"VALIDATION_REQUIRED\",\n",
            "        \"metadata\": {\n",
            "          \"validation_url_link_text\": \"Verify\",\n",
            "          \"validation_url\": \"https://accounts.google.com/signin/continue?sarp=1\\u0026continue=https%3A%2F%2Fx\\u0026plt=AKgn\"\n",
            "        }\n",
            "      }\n",
            "    ]\n",
            "  }\n",
            "}\n",
            "I0925 11:58:01.000000 1 server.go:1] after\n",
        );
        let v = verify_in(log).expect("found");
        assert_eq!(
            v.url,
            "https://accounts.google.com/signin/continue?sarp=1&continue=https%3A%2F%2Fx&plt=AKgn"
        );
        assert_eq!(v.ago, Duration::from_secs(120));
    }

    /// The body logged as one quoted string, and Go's struct printing.
    #[test]
    fn a_verification_link_is_read_escaped_once_more_and_bare() {
        let quoted = "E0925 11:59:00.000000 1 x.go:1] details: \"{\\\"error\\\":{\\\"details\\\":[{\\\"reason\\\":\\\"VALIDATION_REQUIRED\\\",\\\"metadata\\\":{\\\"validation_url\\\":\\\"https://accounts.google.com/v?a=1\\\\u0026b=2\\\"}}]}}\"\n";
        assert_eq!(
            verify_in(quoted).map(|v| v.url).as_deref(),
            Some("https://accounts.google.com/v?a=1&b=2")
        );
        let bare = "E0925 11:59:30.000000 1 x.go:1] {Reason:VALIDATION_REQUIRED Metadata:{ValidationURL:https://accounts.google.com/v?a=1&b=2 UIMessage:}}\n";
        let v = verify_in(bare).expect("found");
        assert_eq!(v.url, "https://accounts.google.com/v?a=1&b=2");
        assert_eq!(v.ago, Duration::from_secs(30));
    }

    /// The camelCase field of an ineligible tier at sign-in, newest wins.
    #[test]
    fn the_newest_verification_link_wins() {
        let log = concat!(
            "E0925 11:10:00.000000 1 x.go:1] {\"reasonCode\": \"VALIDATION_REQUIRED\", \"validationUrl\": \"https://accounts.google.com/old\"}\n",
            "E0925 11:50:00.000000 1 x.go:1] {\"reasonCode\": \"VALIDATION_REQUIRED\", \"validationUrl\": \"https://accounts.google.com/new\"}\n",
        );
        assert_eq!(
            verify_in(log).map(|v| v.url).as_deref(),
            Some("https://accounts.google.com/new")
        );
    }

    /// Only a Google page, only next to its reason, only a page that can be
    /// placed in time.
    #[test]
    fn a_link_that_is_not_a_verification_demand_is_ignored() {
        let foreign = "E0925 11:58:00.000000 1 x.go:1] {\"reason\": \"VALIDATION_REQUIRED\", \"validation_url\": \"https://accounts.google.com.evil.example/x\"}\n";
        assert_eq!(verify_in(foreign), None);
        let userinfo = "E0925 11:58:00.000000 1 x.go:1] {\"reason\": \"VALIDATION_REQUIRED\", \"validation_url\": \"https://accounts.google.com@evil.example/x\"}\n";
        assert_eq!(verify_in(userinfo), None);
        let plain = "E0925 11:58:00.000000 1 x.go:1] {\"reason\": \"VALIDATION_REQUIRED\", \"validation_url\": \"http://accounts.google.com/x\"}\n";
        assert_eq!(verify_in(plain), None);
        let other_reason = "E0925 11:58:00.000000 1 x.go:1] {\"reason\": \"VALIDATION_REQUIRED_SOON\", \"validation_url\": \"https://accounts.google.com/x\"}\n";
        assert_eq!(verify_in(other_reason), None);
        let unstamped = "{\"reason\": \"VALIDATION_REQUIRED\", \"validation_url\": \"https://accounts.google.com/x\"}\n";
        assert_eq!(verify_in(unstamped), None);
        let old = "E0925 09:00:00.000000 1 x.go:1] {\"reason\": \"VALIDATION_REQUIRED\", \"validation_url\": \"https://accounts.google.com/x\"}\n";
        assert_eq!(verify_in(old), None);
    }

    #[test]
    fn test_extract_model_and_response_id() {
        let line = "I0912 15:16:02.123 streamGenerateContent?alt=sse ResponseID: resp-89ab12cd model: models/gemini-2.5-pro";
        assert_eq!(extract_model_name(line), "Gemini 2.5 Pro");
        assert_eq!(extract_response_id(line), Some("resp-89ab12cd".to_string()));

        let flash_line = "I0912 15:16:02.123 streamGenerateContent ResponseID: resp-4444 gemini-2.5-flash";
        assert_eq!(extract_model_name(flash_line), "Gemini 2.5 Flash");
        assert_eq!(extract_response_id(flash_line), Some("resp-4444".to_string()));

        let refusal_line = "E0912 15:16:02.123 User location is not supported for the API use";
        assert_eq!(count_refusals(refusal_line.as_bytes()), 1);
    }

use super::*;
use std::fs;

    #[test]
    fn test_current_version_not_empty() {
        assert!(!current_version().is_empty());
    }

    #[test]
    fn test_version_comparison() {
        assert!(is_newer_version("2.11.0_1", "2.11.0"));
        assert!(is_newer_version("v2.11.0_4", "2.11.0_1"));
        assert!(is_newer_version("2.12.0", "2.11.0_5"));
        assert!(is_newer_version("v3.0.0", "2.11.0"));
        assert!(is_newer_version("2.17.0.3", "2.17.0"));
        assert!(!is_newer_version("2.17.0.3", "2.17.0.3"));
        assert!(!is_newer_version("2.17.0", "2.17.0.3"));
        assert!(!is_newer_version("2.11.0", "2.11.0"));
        assert!(!is_newer_version("v2.11.0", "2.11.0"));
        assert!(!is_newer_version("2.11.0_1", "2.11.0_4"));
        assert!(!is_newer_version("2.10.0", "2.11.0"));
    }

    #[test]
    fn test_update_info_json_deserialization() {
        let sample = r#"{
            "version": "1.1.2",
            "notes": "Р С•РЎвЂљРЎвЂЎРЎвЂРЎвЂљ Р Р† data/, Р С”Р Р…Р С•Р С—Р С”Р В° Р’В«Р вЂ”Р В°Р Р†Р ВµРЎР‚РЎв‚¬Р С‘РЎвЂљРЎРЉ Р С—РЎР‚Р С•РЎвЂ Р ВµРЎРѓРЎРѓР’В»",
            "page": "https://github.com/keelbismark/OpenAntigravity",
            "url_windows": "https://example.com/OpenAntigravity_windows_v1.1.2.zip",
            "url_linux": "https://example.com/OpenAntigravity_linux_v1.1.2.tar.gz"
        }"#;

        let info: UpdateInfo = serde_json::from_str(sample).expect("valid json");
        assert_eq!(info.display_version(), "1.1.2");
        assert!(info.is_newer_than_current() == is_newer_version("1.1.2", current_version()));
        assert_eq!(info.url_windows.as_deref(), Some("https://example.com/OpenAntigravity_windows_v1.1.2.zip"));
    }

    #[test]
    fn test_update_info_platform_download_url_prefers_portable_zip() {
        let sample = r#"{
            "version": "1.4.1",
            "notes": "Open Antigravity v1.4.1",
            "page": "https://github.com/keelbismark/OpenAntigravity/releases/tag/v1.4.1",
            "url_windows": "https://github.com/keelbismark/OpenAntigravity/releases/download/v1.4.1/OpenAntigravity_windows_v1.4.1_setup.exe",
            "url_windows_setup": "https://github.com/keelbismark/OpenAntigravity/releases/download/v1.4.1/OpenAntigravity_windows_v1.4.1_setup.exe",
            "url_windows_portable": "https://github.com/keelbismark/OpenAntigravity/releases/download/v1.4.1/OpenAntigravity_windows_v1.4.1_portable.zip",
            "url_linux": "https://github.com/keelbismark/OpenAntigravity/releases/download/v1.4.1/OpenAntigravity_linux_v1.4.1.tar.gz",
            "url_appimage": "https://github.com/keelbismark/OpenAntigravity/releases/download/v1.4.1/OpenAntigravity-x86_64.AppImage"
        }"#;

        let info: UpdateInfo = serde_json::from_str(sample).expect("valid json");
        #[cfg(target_os = "windows")]
        {
            assert_eq!(
                info.platform_download_url(),
                Some("https://github.com/keelbismark/OpenAntigravity/releases/download/v1.4.1/OpenAntigravity_windows_v1.4.1_portable.zip")
            );
            assert_eq!(
                info.landing_url(),
                Some("https://github.com/keelbismark/OpenAntigravity/releases/download/v1.4.1/OpenAntigravity_windows_v1.4.1_setup.exe")
            );
        }
    }

    #[test]
    fn test_update_info_optional_fields_default() {
        // Only `version` is required; a hand-edited feed without the rest
        // must still parse.
        let info: UpdateInfo =
            serde_json::from_str(r#"{ "version": "v3.0.0" }"#).expect("minimal json");
        assert_eq!(info.display_version(), "3.0.0");
        assert!(info.notes.is_none());
        assert!(info.landing_url().is_none());
    }

    #[test]
    fn test_parse_url() {
        let p = parse_url("https://example.com/feed/version.json").expect("https url");
        assert!(p.tls);
        assert_eq!(p.host, "example.com");
        assert_eq!(p.port, 443);
        assert_eq!(p.path, "/feed/version.json");

        let p = parse_url("http://10.0.0.1:8080/").expect("http url with port");
        assert!(!p.tls);
        assert_eq!(p.host, "10.0.0.1");
        assert_eq!(p.port, 8080);
        assert_eq!(p.path, "/");

        let p = parse_url("http://[::1]:9000/v.json").expect("ipv6 literal");
        assert_eq!(p.host, "::1");
        assert_eq!(p.port, 9000);

        let p = parse_url("https://example.com").expect("no path");
        assert_eq!(p.path, "/");

        assert!(parse_url("ftp://example.com/x").is_err());
        assert!(parse_url("example.com/x").is_err());
        assert!(parse_url("http://host:notaport/").is_err());
    }

    #[test]
    fn test_resolve_redirect() {
        let base = parse_url("http://old.example.com/feed/version.json").expect("base");
        let abs = resolve_redirect(&base, "https://new.example.com/v.json").expect("absolute");
        assert_eq!(abs.host, "new.example.com");
        assert!(abs.tls);

        let rel = resolve_redirect(&base, "/feed/v2.json").expect("root-relative");
        assert_eq!(rel.host, "old.example.com");
        assert_eq!(rel.path, "/feed/v2.json");

        let bare = resolve_redirect(&base, "v2.json").expect("bare relative");
        assert_eq!(bare.path, "/feed/v2.json");
    }

    #[test]
    fn test_landing_url_prefers_own_platform() {
        let info = UpdateInfo {
            version: "1.0.0".into(),
            notes: None,
            date: None,
            page: Some("https://example.com/page".into()),
            url_windows: Some("https://example.com/win.zip".into()),
            url_windows_setup: None,
            url_windows_portable: None,
            url_linux: Some("https://example.com/linux.tar.gz".into()),
            url_appimage: Some("https://example.com/appimage".into()),
            sha256_windows: None,
            sha256_linux: None,
            sha256_appimage: None,
            sha256: None,
        };
        let landing = info.landing_url().expect("some landing").to_string();
        if cfg!(target_os = "windows") {
            assert_eq!(landing, "https://example.com/win.zip");
            assert_eq!(info.platform_download_url(), Some("https://example.com/win.zip"));
        } else {
            assert!(landing.contains("example.com"));
            assert!(info.platform_download_url().is_some());
        }
    }

    #[test]
    fn test_decode_chunked() {
        let chunked = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let decoded = decode_chunked(chunked).expect("chunked decode");
        assert_eq!(decoded, b"hello world");
    }

    #[test]
    fn test_parse_response_status_and_location() {
        let raw = b"HTTP/1.1 302 Found\r\nLocation: https://example.com/new\r\nContent-Length: 0\r\n\r\n";
        let (status, location, body) = parse_response(raw).expect("parse");
        assert_eq!(status, 302);
        assert_eq!(location.as_deref(), Some("https://example.com/new"));
        assert!(body.is_empty());

        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"version\":\"1\"}";
        let (status, location, body) = parse_response(raw).expect("parse");
        assert_eq!(status, 200);
        assert!(location.is_none());
        assert_eq!(body, b"{\"version\":\"1\"}");
    }

    #[test]
    fn test_tls_config_init() {
        let cfg = tls_config();
        assert_eq!(cfg.alpn_protocols, vec![b"http/1.1".to_vec()]);
    }

    #[test]
    fn test_baked_update_url() {
        let url = update_url();
        assert!(url.is_some(), "baked update url must not be empty");
        assert!(url.unwrap().contains("version.json"));
    }

    #[test]
    #[ignore = "performs a real network request; point OAG_UPDATE_URL at a live feed to exercise the happy path"]
    fn test_live_fetch_update_info() {
        let url = std::env::var("OAG_UPDATE_URL")
            .unwrap_or_else(|_| "http://localhost:1/version.json".to_string());
        std::env::set_var("OAG_UPDATE_URL", &url);
        match fetch_update_info() {
            Ok(info) => {
                // A live feed answered: the document must at least name a version.
                assert!(!info.version.is_empty(), "feed returned an empty version");
            }
            Err(e) => {
                // The default target deliberately listens on nothing РІР‚вЂќ the
                // point is that the whole path runs and fails honestly.
                assert!(
                    url.contains("localhost:1"),
                    "live fetch failed against a real feed: {e}"
                );
            }
        }
    }

    #[test]
    fn test_unique_old_file_replacement_cycle() {
        let dir = std::env::temp_dir().join(format!("oag_test_repl_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let current = dir.join("app.exe");
        let new_bin = dir.join("app_new.exe");
        fs::write(&current, b"version1").unwrap();
        fs::write(&new_bin, b"version2").unwrap();

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let old = current.with_extension(format!("exe.old.{}", stamp));

        assert!(fs::rename(&current, &old).is_ok());
        assert!(fs::copy(&new_bin, &current).is_ok());
        assert_eq!(fs::read(&current).unwrap(), b"version2");
        assert_eq!(fs::read(&old).unwrap(), b"version1");
        assert!(fs::remove_file(&old).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_compute_and_verify_sha256() {
        let temp_file = std::env::temp_dir().join(format!("oag_test_sha_{}.bin", std::process::id()));
        fs::write(&temp_file, b"Open Antigravity Update Test Payload").unwrap();

        // sha256 of "Open Antigravity Update Test Payload" is:
        // echo -n "Open Antigravity Update Test Payload" | sha256sum
        // let's compute it with our function:
        let hash = compute_file_sha256(&temp_file).expect("compute sha256");
        assert_eq!(hash.len(), 64);
        assert!(verify_file_sha256(&temp_file, &hash).is_ok());
        assert!(verify_file_sha256(&temp_file, &hash.to_uppercase()).is_ok());
        assert!(verify_file_sha256(&temp_file, "0000000000000000000000000000000000000000000000000000000000000000").is_err());

        let _ = fs::remove_file(&temp_file);
    }

    #[test]
    fn test_expected_sha256_resolution() {
        let mut info = UpdateInfo {
            version: "2.0.0".into(),
            notes: None,
            date: None,
            page: None,
            url_windows: None,
            url_windows_setup: None,
            url_windows_portable: None,
            sha256_windows: Some("abc_win".into()),
            url_linux: None,
            sha256_linux: Some("abc_linux".into()),
            url_appimage: None,
            sha256_appimage: Some("abc_appimage".into()),
            sha256: Some("abc_generic".into()),
        };

        if cfg!(target_os = "windows") {
            assert_eq!(info.expected_sha256(), Some("abc_win"));
            info.sha256_windows = None;
            assert_eq!(info.expected_sha256(), Some("abc_generic"));
        } else {
            assert!(info.expected_sha256().is_some());
        }
    }

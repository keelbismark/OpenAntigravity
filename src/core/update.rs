//! New-version check against the build's own update feed.
//!
//! The feed is a small JSON document (`version.json`) served by whoever
//! distributes this build — by default the same site the archive came from.
//! Its address is baked at compile time with `AG_UPDATE_URL` and can be
//! overridden at runtime with the same variable, so one binary can be pointed
//! at a mirror without a rebuild:
//!
//! ```json
//! {
//!   "version": "1.1.2",
//!   "notes": "что нового",
//!   "page": "https://host/страница-проекта",
//!   "url_windows": "https://host/OpenAntigravity_windows_v1.1.2.zip",
//!   "url_linux":   "https://host/OpenAntigravity_linux_v1.1.2.tar.gz"
//! }
//! ```
//!
//! Only `version` is required; every other field is optional and a missing
//! download link just means the banner falls back to `page`. When
//! `AG_UPDATE_URL` is neither baked nor set at runtime the whole subsystem is
//! inert: no thread, no requests, no banner — a build shipped to nobody in
//! particular must not phone a feed its distributor never chose.
//!
//! Synchronous on purpose, like the rest of the crate: one HTTP/1.1 GET, over
//! rustls for `https://` and plain TCP for `http://` (a home VPS behind
//! Dokploy often has no certificate yet), the same pattern `doh.rs` and
//! `proxy.rs` already use. The GUI runs checks on their own thread and never
//! on the paint loop. Redirects (301–308) are followed, up to a small limit —
//! http→https upgrades and trailing-slash fixes are the norm, not the
//! exception, on such setups.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection};
use serde::{Deserialize, Serialize};

const CHECK_INTERVAL: Duration = Duration::from_secs(8 * 60 * 60);

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY: usize = 256 * 1024;
const MAX_REDIRECTS: usize = 3;

/// The version this binary shipped as, in the form the feed's `version` field
/// uses.
///
/// Read from `AG_FULL_VERSION` (set by build.rs from the `VERSION` file) or falls back
/// to `CARGO_PKG_VERSION` from Cargo.toml.
pub fn current_version() -> &'static str {
    option_env!("AG_FULL_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// The feed address: runtime `AG_UPDATE_URL` wins over the value baked at
/// build time. Empty means "unset" in both places — `AG_UPDATE_URL=` at
/// runtime can therefore silence a baked-in feed, which is the only way a
/// misbehaving mirror can be switched off without a rebuild.
pub fn update_url() -> Option<String> {
    if let Ok(v) = std::env::var("AG_UPDATE_URL") {
        let t = v.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    option_env!("AG_UPDATE_URL")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The document the feed serves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    /// Newest shipped version, e.g. `2.17.0.3` (a leading `v` is tolerated).
    pub version: String,
    /// One line on what changed; shown where the UI has room for it.
    #[serde(default)]
    pub notes: Option<String>,
    /// Fallback link — the project page — opened when no archive matches the
    /// running platform.
    #[serde(default)]
    pub page: Option<String>,
    /// Archive for Windows builds.
    #[serde(default)]
    pub url_windows: Option<String>,
    /// Archive for Linux/SteamOS builds.
    #[serde(default)]
    pub url_linux: Option<String>,
}

impl UpdateInfo {
    /// Strips leading 'v' / 'V' and whitespace for display.
    pub fn display_version(&self) -> &str {
        self.version
            .trim()
            .trim_start_matches(['v', 'V'])
    }

    /// Returns true if this feed entry is strictly newer than the running binary.
    pub fn is_newer_than_current(&self) -> bool {
        is_newer_version(&self.version, current_version())
    }

    /// Where a click on the banner should land: this platform's archive first,
    /// the other platform's as a distant second (a Linux user handed the
    /// Windows link can still see the page it came from), the project page
    /// last.
    pub fn landing_url(&self) -> Option<&str> {
        #[cfg(target_os = "windows")]
        {
            self.url_windows
                .as_deref()
                .or(self.url_linux.as_deref())
                .or(self.page.as_deref())
        }
        #[cfg(not(target_os = "windows"))]
        {
            self.url_linux
                .as_deref()
                .or(self.url_windows.as_deref())
                .or(self.page.as_deref())
        }
    }
}

/// Cached check payload stored on disk. The URL is part of the key: a cache
/// filled from yesterday's mirror must not answer for today's.
#[derive(Debug, Serialize, Deserialize)]
struct CachedCheck {
    checked_at_unix: u64,
    url: String,
    info: UpdateInfo,
}

fn cache_path() -> PathBuf {
    std::env::temp_dir().join("open_antigravity_update_feed.json")
}

/// Compares two version strings (e.g. "2.11.0_4" vs "2.11.0", "2.17.0.3" vs
/// "2.17.0"). Returns true if `remote` is strictly newer than `current`.
pub fn is_newer_version(remote: &str, current: &str) -> bool {
    let parse_segments = |v: &str| -> Vec<u64> {
        let clean = v.trim().trim_start_matches(['v', 'V']);
        clean
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<u64>().ok())
            .collect()
    };

    let r_parts = parse_segments(remote);
    let c_parts = parse_segments(current);

    let max_len = r_parts.len().max(c_parts.len());
    for i in 0..max_len {
        let r = r_parts.get(i).copied().unwrap_or(0);
        let c = c_parts.get(i).copied().unwrap_or(0);
        if r > c {
            return true;
        } else if r < c {
            return false;
        }
    }
    false
}

/// Client configuration for TLS. Reuses the project's standard pattern with WebPKI roots
/// and HTTP/1.1 ALPN protocol.
fn tls_config() -> Arc<ClientConfig> {
    static CFG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CFG.get_or_init(|| {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let mut cfg = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
        Arc::new(cfg)
    })
    .clone()
}

/// A feed URL taken apart into the pieces one HTTP request needs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedUrl {
    tls: bool,
    host: String,
    port: u16,
    path: String,
}

/// Parses `http(s)://host[:port]/path`. Everything else is rejected with a
/// message meant to be shown to the person who typed the variable, not logged
/// away.
fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    let url = url.trim();
    let (tls, rest) = if let Some(r) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("HTTPS://"))
    {
        (true, r)
    } else if let Some(r) = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("HTTP://"))
    {
        (false, r)
    } else {
        return Err(format!(
            "URL должен начинаться с http:// или https:// (сейчас: {url})"
        ));
    };

    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if hostport.is_empty() {
        return Err("в URL нет хоста".to_string());
    }
    let path = if path.is_empty() { "/" } else { path };

    // IPv6 literals come in brackets; split on the last ':' outside them.
    let (host, port) = if let Some(end) = hostport.strip_prefix('[') {
        match end.split_once(']') {
            Some((h, tail)) => {
                let port = match tail.strip_prefix(':') {
                    Some(p) => p.parse::<u16>().map_err(|_| "неверный порт в URL")?,
                    None => 0,
                };
                (h.to_string(), port)
            }
            None => return Err("незакрытая скобка IPv6 в URL".to_string()),
        }
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (
                h.to_string(),
                p.parse::<u16>().map_err(|_| "неверный порт в URL")?,
            ),
            None => (hostport.to_string(), 0),
        }
    };

    Ok(ParsedUrl {
        tls,
        host,
        port: if port == 0 {
            if tls {
                443
            } else {
                80
            }
        } else {
            port
        },
        path: path.to_string(),
    })
}

/// Connects a TCP stream to `host:port` with a connection timeout.
fn connect_tcp(host: &str, port: u16) -> Result<TcpStream, String> {
    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("DNS resolution failed for {}: {}", host, e))?
        .collect();

    let mut last_err = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(sock) => {
                sock.set_read_timeout(Some(IO_TIMEOUT)).ok();
                sock.set_write_timeout(Some(IO_TIMEOUT)).ok();
                return Ok(sock);
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(format!(
        "could not connect to {}:{}: {}",
        host,
        port,
        last_err.map_or_else(|| "no address resolved".to_string(), |e| e.to_string())
    ))
}

/// Decodes HTTP chunked transfer encoding if present in response.
fn decode_chunked(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while cursor < raw.len() {
        let rem = &raw[cursor..];
        let Some(pos) = rem.windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let line = std::str::from_utf8(&rem[..pos])
            .map_err(|e| format!("invalid chunk size encoding: {}", e))?
            .trim();
        let chunk_size = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|e| format!("invalid chunk size hex '{}': {}", line, e))?;
        if chunk_size == 0 {
            break;
        }
        let chunk_start = cursor + pos + 2;
        // Checked: the size comes off the wire, and `chunk_start + chunk_size`
        // on a hostile or corrupt value overflows. `panic = "abort"` makes an
        // overflow panic a dead window rather than a caught error.
        let Some(chunk_end) = chunk_start.checked_add(chunk_size) else {
            return Err("chunk size out of range".to_string());
        };
        if chunk_end > raw.len() {
            return Err("truncated chunk data in HTTP response".to_string());
        }
        out.extend_from_slice(&raw[chunk_start..chunk_end]);
        cursor = chunk_end + 2; // skip trailing \r\n
    }
    Ok(out)
}

/// One HTTP/1.1 GET against an already-parsed URL. Returns the status code,
/// the `Location` header if any, and the body.
fn http_request(url: &ParsedUrl) -> Result<(u16, Option<String>, Vec<u8>), String> {
    let mut sock = connect_tcp(&url.host, url.port)?;

    if url.tls {
        let server_name = ServerName::try_from(url.host.clone())
            .map_err(|e| format!("invalid TLS server name {}: {}", url.host, e))?;
        let mut conn = ClientConnection::new(tls_config(), server_name)
            .map_err(|e| format!("failed to initialize TLS client connection: {}", e))?;
        let mut stream = rustls::Stream::new(&mut conn, &mut sock);
        let body = roundtrip(&mut stream, url)?;
        Ok(parse_response(&body)?)
    } else {
        let body = roundtrip(&mut sock, url)?;
        Ok(parse_response(&body)?)
    }
}

/// Writes the request and drains the answer. Generic over rustls' TLS wrapper
/// and the bare TCP socket, which expose the same Read+Write surface.
fn roundtrip<S: Read + Write>(stream: &mut S, url: &ParsedUrl) -> Result<Vec<u8>, String> {
    let req = format!(
        "GET {} HTTP/1.1\r\n\
         Host: {}\r\n\
         User-Agent: open_antigravity/{}\r\n\
         Accept: application/json\r\n\
         Connection: close\r\n\r\n",
        url.path,
        url.host,
        current_version()
    );

    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("failed to write HTTP request: {}", e))?;
    stream
        .flush()
        .map_err(|e| format!("failed to flush request: {}", e))?;

    let mut response_buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                response_buf.extend_from_slice(&chunk[..n]);
                if response_buf.len() > MAX_BODY {
                    return Err(format!("response body exceeded limit ({} bytes)", MAX_BODY));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // Connection closed without TLS close_notify alert
                break;
            }
            Err(e) => return Err(format!("error reading HTTP response: {}", e)),
        }
    }

    if response_buf.is_empty() {
        return Err("empty response received from the update feed".to_string());
    }
    Ok(response_buf)
}

/// Splits an HTTP answer into (status, Location, body), decoding chunked
/// transfer encoding when the server chose it.
fn parse_response(raw: &[u8]) -> Result<(u16, Option<String>, Vec<u8>), String> {
    let header_delim = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "missing header delimiter in HTTP response".to_string())?;

    let header_str = String::from_utf8_lossy(&raw[..header_delim]);
    let body_raw = &raw[header_delim + 4..];

    let mut lines = header_str.lines();
    let status_line = lines.next().unwrap_or("");
    // "HTTP/1.1 200 OK" — the code is the second token, not a substring hunt:
    // " 200" also matches "1200" and a proxy's " 2001".
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| format!("unreadable status line: {status_line}"))?;

    let mut location = None;
    let mut is_chunked = false;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("location:") {
            location = Some(line[9..].trim().to_string());
        } else if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            is_chunked = true;
        }
    }

    let body = if is_chunked {
        decode_chunked(body_raw)?
    } else {
        body_raw.to_vec()
    };
    Ok((status, location, body))
}

/// Resolves a `Location` against the URL it came from. Absolute URLs pass
/// through; relative ones are joined the way servers actually mean them
/// (scheme and host kept, path replaced unless it starts with `/`).
fn resolve_redirect(base: &ParsedUrl, location: &str) -> Result<ParsedUrl, String> {
    let loc = location.trim();
    if loc.starts_with("http://") || loc.starts_with("https://") {
        parse_url(loc)
    } else if let Some(rest) = loc.strip_prefix('/') {
        Ok(ParsedUrl {
            path: format!("/{rest}"),
            ..base.clone()
        })
    } else {
        // A bare relative path ("feed.json") — replace the last segment.
        let up_to = base.path.rfind('/').unwrap_or(0);
        Ok(ParsedUrl {
            path: format!("{}/{}", &base.path[..up_to], loc),
            ..base.clone()
        })
    }
}

/// Synchronously fetches and parses the feed, following redirects.
pub fn fetch_update_info() -> Result<UpdateInfo, String> {
    let url = update_url()
        .ok_or_else(|| "URL проверки обновлений не задан (AG_UPDATE_URL)".to_string())?;
    let mut current = parse_url(&url)?;

    for _ in 0..=MAX_REDIRECTS {
        let (status, location, body) = http_request(&current)?;
        match status {
            200 => {
                let info: UpdateInfo = serde_json::from_slice(&body).map_err(|e| {
                    format!(
                        "failed to parse version.json ({}): {}",
                        String::from_utf8_lossy(body.get(..200.min(body.len())).unwrap_or(&[])),
                        e
                    )
                })?;
                return Ok(info);
            }
            301 | 302 | 303 | 307 | 308 => {
                let loc = location
                    .ok_or_else(|| format!("сервер вернул {status} без Location"))?;
                current = resolve_redirect(&current, &loc)?;
            }
            s => {
                return Err(format!(
                    "сервер обновлений вернул статус {s} ({}:{})",
                    current.host, current.port
                ))
            }
        }
    }
    Err(format!("слишком много перенаправлений (>{MAX_REDIRECTS})"))
}

/// Checks the feed, honouring `CHECK_INTERVAL` (8 hours) via the on-disk
/// cache. `force = true` skips the cache — that is what the manual «Проверить
/// обновления» button uses. An update returns `Ok(Some(info))`; a feed that
/// matches the running version returns `Ok(None)`.
pub fn check_update_cached(force: bool) -> Result<Option<UpdateInfo>, String> {
    let url = update_url()
        .ok_or_else(|| "URL проверки обновлений не задан (AG_UPDATE_URL)".to_string())?;
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let path = cache_path();
    if !force {
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(cached) = serde_json::from_str::<CachedCheck>(&data) {
                if cached.url == url
                    && now_unix >= cached.checked_at_unix
                    && Duration::from_secs(now_unix - cached.checked_at_unix) < CHECK_INTERVAL
                {
                    return if cached.info.is_newer_than_current() {
                        Ok(Some(cached.info))
                    } else {
                        Ok(None)
                    };
                }
            }
        }
    }

    let info = fetch_update_info()?;
    let cache_entry = CachedCheck {
        checked_at_unix: now_unix,
        url: url.clone(),
        info: info.clone(),
    };
    if let Ok(serialized) = serde_json::to_string(&cache_entry) {
        fs::write(&path, serialized).ok();
    }

    if info.is_newer_than_current() {
        Ok(Some(info))
    } else {
        Ok(None)
    }
}

/// The manual check the «Проверить обновления» button runs: always on the
/// wire, never answered from the cache. Its result goes to a place the user
/// is looking at, so errors are returned, not swallowed.
pub fn manual_check() -> Result<Option<UpdateInfo>, String> {
    check_update_cached(true)
}

/// Watches the feed for as long as the receiver lives.
///
/// Checked once immediately — the banner has to be up before the licence
/// screen is even answered — and then every `CHECK_INTERVAL` for a window
/// that stays open. Only the first pass may answer from the on-disk cache:
/// after that the thread has already waited the full interval, so re-reading
/// a cache it wrote itself would just double the wait. Inert unless
/// `update_url()` says where to look.
pub fn spawn_watch(tx: std::sync::mpsc::Sender<UpdateInfo>, wake: Box<dyn Fn() + Send>) {
    if update_url().is_none() {
        return;
    }
    std::thread::Builder::new()
        .name("update-watch".to_string())
        .spawn(move || {
            let mut first = true;
            loop {
                match check_update_cached(!first) {
                    Ok(Some(info)) => {
                        // A closed receiver means the window is gone; so is the
                        // reason to keep checking.
                        if tx.send(info).is_err() {
                            return;
                        }
                        // egui sleeps until something asks it to repaint, so a
                        // banner that only lands in a channel stays invisible
                        // until the user happens to move the mouse.
                        wake();
                    }
                    Ok(None) => {}
                    Err(_e) => {
                        // Background check: a failure is not the user's problem,
                        // and there is no UI surface that could act on it.
                        #[cfg(debug_assertions)]
                        eprintln!("update check failed: {}", _e);
                    }
                }
                first = false;
                std::thread::sleep(CHECK_INTERVAL);
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "notes": "отчёт в data/, кнопка «Завершить процесс»",
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
            page: Some("https://example.com/page".into()),
            url_windows: Some("https://example.com/win.zip".into()),
            url_linux: Some("https://example.com/linux.tar.gz".into()),
        };
        let landing = info.landing_url().expect("some landing").to_string();
        if cfg!(target_os = "windows") {
            assert_eq!(landing, "https://example.com/win.zip");
        } else {
            assert_eq!(landing, "https://example.com/linux.tar.gz");
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
    #[ignore = "performs a real network request; point AG_UPDATE_URL at a live feed to exercise the happy path"]
    fn test_live_fetch_update_info() {
        let url = std::env::var("AG_UPDATE_URL")
            .unwrap_or_else(|_| "http://localhost:1/version.json".to_string());
        std::env::set_var("AG_UPDATE_URL", &url);
        match fetch_update_info() {
            Ok(info) => {
                // A live feed answered: the document must at least name a version.
                assert!(!info.version.is_empty(), "feed returned an empty version");
            }
            Err(e) => {
                // The default target deliberately listens on nothing — the
                // point is that the whole path runs and fails honestly.
                assert!(
                    url.contains("localhost:1"),
                    "live fetch failed against a real feed: {e}"
                );
            }
        }
    }
}

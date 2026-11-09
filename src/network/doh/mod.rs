use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use rustls::client::Resumption;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, HandshakeKind};

// DNS-over-HTTPS, over a hand-written HTTP/2 client.
//
// Why hand-written. The resolver this exists for answers `505 HTTP Version Not
// Supported` to HTTP/1.1 and offers only `h2` in ALPN - measured on both of its
// addresses. The crate is synchronous and has no HTTP client at all, so the
// alternatives were the `h2` crate (which brings tokio, i.e. an async runtime,
// into a codebase that has none) or the subset of HTTP/2 a DoH exchange
// actually uses. It is a small subset, because one DoH query is one request and
// one response, and a connection carries its queries one after another:
//
// - no multiplexing: one stream at a time per connection, ids 1, 3, 5, ... A
//   connection is kept for the next query (`Pool`), never shared by two at once;
//   a thread that finds none free opens its own.
// - flow control, once: right after the preface the connection-level window is
//   raised to the maximum, so the server never runs out of it however many
//   replies one connection carries. Each stream's own window starts at 65 535,
//   which holds any DNS message, so no per-stream WINDOW_UPDATE is ever needed.
// - no dynamic HPACK table: the request is five headers, and nothing is sent
//   with indexing - on a long connection every unique `?dns=` would otherwise
//   churn the server's table. The *response* headers are never decoded at all,
//   so our decoder's state does not matter. Only DATA frames on the stream
//   being asked are read. The status code is not needed - an error page does
//   not parse as a DNS message, and every caller already handles "no usable
//   reply".
//
// What is deliberately NOT skipped: SETTINGS and PING are acknowledged (a server
// is entitled to drop a connection that ignores them), and GOAWAY/RST_STREAM are
// reported rather than waited out.
//
// Why connections are kept at all: dns-ai.ru measured (2026-09-18) ~195 new
// TCP connections and ~175 DoH handshakes a second per node, 1.06 queries per
// connection, and its CPU going mostly on TLS handshakes. One fresh connection
// per query was the design here, so this client paid for its share of that in
// full.
//
// Pinned to the ISP interface while a tunnel holds the default route, like every
// UDP resolver query (`dns_client::query_raw_via` + IP_UNICAST_IF, I4/N4), so a
// VPN cannot change the geolocation the provider sees. IP_UNICAST_IF has to be
// set *before* connect, which is why the socket comes from `net::connect`
// rather than `TcpStream::connect_timeout` (P13, closed in 2.14.0_1).

/// A DoH service: the name its certificate has to prove, the path that answers
/// RFC 8484 queries, and addresses to reach it at.
pub struct Endpoint {
    pub host: &'static str,
    pub path: &'static str,
    /// Hardcoded so the relay reaches the resolver on a machine whose own DNS is
    /// broken, poisoned, or simply not up yet - the same trick the relay route
    /// uses, and safe for the same reason: the certificate still has to prove
    /// `host`, so a wrong address cannot become a working man-in-the-middle.
    pub addrs: &'static [&'static str],
}

/// One address gets this much for connect + TLS + the exchange.
///
/// Sliced per candidate rather than shared across them: an address that
/// completes TCP and then hangs the handshake would otherwise spend the whole
/// budget and leave its healthy sibling untried - which is G23, one layer up.
const PER_ADDR_BUDGET: Duration = Duration::from_millis(2500);

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

const FRAME_DATA: u8 = 0x0;
const FRAME_HEADERS: u8 = 0x1;
const FRAME_RST_STREAM: u8 = 0x3;
const FRAME_SETTINGS: u8 = 0x4;
const FRAME_PING: u8 = 0x6;
const FRAME_GOAWAY: u8 = 0x7;
const FRAME_WINDOW_UPDATE: u8 = 0x8;

const FLAG_END_STREAM: u8 = 0x1;
const FLAG_ACK: u8 = 0x1;
const FLAG_END_HEADERS: u8 = 0x4;

/// A DNS reply that does not fit this is not a reply we would use.
const MAX_BODY: usize = 64 * 1024;

/// Raises the connection window from its initial 65 535 to 2^31-1, the most
/// RFC 7540 В§6.9.1 allows. Without it the window is 65 535 bytes for the WHOLE
/// connection, never refilled: after ~65 KB of replies in total the server
/// stops sending DATA and the next query hangs until its budget runs out.
const CONN_WINDOW_INCREMENT: u32 = 0x7FFF_FFFF - 65_535;

/// A connection idle this long is not taken from the pool. dns-ai.ru's dnsdist
/// closes a DoH connection idle for 30 s (`idleTimeout = 30`); 20 leaves room
/// for a request that leaves just as the server's clock runs out. The warm
/// loop asks every ~15 s, which is what keeps a connection warm in practice.
const IDLE_LIMIT: Duration = Duration::from_secs(20);

/// Requests one connection carries before it is retired. Not a protocol limit:
/// a bound on how long one TLS session, one HPACK context and one stream-id
/// counter live, and a cheap one - a handshake per 500 queries.
const MAX_REQUESTS: u32 = 500;

/// Idle connections kept per address. More than one thread asks at once (the
/// provider race, the warm loop, the client's own A and AAAA), so one would
/// make every burst open fresh connections; the extras age out by `IDLE_LIMIT`.
const MAX_IDLE_PER_ADDR: usize = 2;

fn base_tls_config() -> &'static ClientConfig {
    static CFG: OnceLock<ClientConfig> = OnceLock::new();
    CFG.get_or_init(|| {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let mut cfg = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        // Not shared with `proxy::upstream_config`, which pins http/1.1 for the
        // relay's CONNECT hop. This one must offer h2 and nothing else: if a
        // server were to pick http/1.1 the frames below would be nonsense on the
        // wire, and failing in ALPN is the honest place to find that out.
        cfg.alpn_protocols = vec![b"h2".to_vec()];
        cfg
    })
}

/// The TLS config for one ADDRESS, not one process: each carries its own
/// session store.
///
/// rustls files tickets under the server name, and every address of an
/// `Endpoint` proves the same name - but they are separate machines, each with
/// its own ticket keys (on purpose; the operator will not share them). One
/// store, and a walk that alternates the nodes, meant every connection
/// presented the ticket the *other* node had issued: the store hands out the
/// newest one first. The node cannot decrypt it and falls back to a full
/// handshake - measured on dns-ai.ru 2026-09-18 as ~26 `tlsunknownticketkeys`
/// a second per node, with resumption almost never happening.
///
/// A store per address makes the server name mean one machine again.
fn tls_config(ip: IpAddr) -> Arc<ClientConfig> {
    static PER_ADDR: OnceLock<Mutex<HashMap<IpAddr, Arc<ClientConfig>>>> = OnceLock::new();
    let fresh = || {
        let mut cfg = base_tls_config().clone();
        // The clone copies the `Arc` of the base config's store, so without this
        // line every address would still share one - and nothing would change.
        //
        // 16 and not fewer, although one name is all a store here ever holds:
        // rustls sizes it in server names, (n + 7) / 8, and a one-name store
        // evicts its only entry the moment it is inserted (`LimitedCache` keeps
        // `len < capacity`). Any n <= 8 therefore resumes nothing - measured,
        // a test asking for 4 saw no node resume at all.
        cfg.resumption = Resumption::in_memory_sessions(16);
        Arc::new(cfg)
    };
    let Ok(mut map) = PER_ADDR.get_or_init(Default::default).lock() else {
        // Poisoned only by a panic inside `fresh`; a config that cannot resume
        // still connects, which is all this path has to guarantee.
        return fresh();
    };
    map.entry(ip).or_insert_with(fresh).clone()
}

/// base64url without padding (RFC 4648 В§5), which is what `?dns=` takes.
fn base64url(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(A[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(A[n as usize & 63] as char);
        }
    }
    out
}

/// HPACK integer (RFC 7541 В§5.1): `prefix` low bits of `first`, then
/// continuation octets. Written out rather than assumed small - a client query
/// carrying EDNS options pushes the encoded path past the 126 bytes a
/// single-octet length can hold, and that would be a corrupt request rather than
/// a slow one.
fn hpack_int(out: &mut Vec<u8>, mut value: usize, prefix: u32, first: u8) {
    let max = (1usize << prefix) - 1;
    if value < max {
        out.push(first | value as u8);
        return;
    }
    out.push(first | max as u8);
    value -= max;
    while value >= 128 {
        out.push(((value % 128) + 128) as u8);
        value /= 128;
    }
    out.push(value as u8);
}

/// HPACK string literal, never Huffman-coded: the H bit stays 0 so the bytes go
/// out as they are. Huffman would save a few dozen bytes on a request that is
/// already one packet.
fn hpack_string(out: &mut Vec<u8>, s: &[u8]) {
    hpack_int(out, s.len(), 7, 0x00);
    out.extend_from_slice(s);
}

/// The five headers of a DoH GET, HPACK-encoded.
fn request_headers(host: &str, path: &str) -> Vec<u8> {
    let mut h = Vec::with_capacity(192);
    // Indexed header field, static table: 2 = `:method GET`, 7 = `:scheme https`.
    h.push(0x82);
    h.push(0x87);
    // Literal WITHOUT indexing, name from the static table (RFC 7541 В§6.2.2).
    // With incremental indexing (0x40) every request on a kept connection would
    // insert its unique `?dns=` path into the server's dynamic table, evicting
    // the last one - work for the server and nothing for us, since we never
    // refer back to the table. `:authority` repeats, but gains no more from it.
    hpack_int(&mut h, 1, 4, 0x00); // :authority
    hpack_string(&mut h, host.as_bytes());
    hpack_int(&mut h, 4, 4, 0x00); // :path
    hpack_string(&mut h, path.as_bytes());
    // Literal without indexing, new name.
    hpack_int(&mut h, 0, 4, 0x00);
    hpack_string(&mut h, b"accept");
    hpack_string(&mut h, b"application/dns-message");
    h
}

fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(9 + payload.len());
    let len = payload.len() as u32;
    f.extend_from_slice(&len.to_be_bytes()[1..]);
    f.push(kind);
    f.push(flags);
    f.extend_from_slice(&stream.to_be_bytes());
    f.extend_from_slice(payload);
    f
}

/// WINDOW_UPDATE (RFC 7540 В§6.9): a 31-bit increment, the top bit reserved.
fn window_update(stream: u32, increment: u32) -> Vec<u8> {
    frame(
        FRAME_WINDOW_UPDATE,
        0,
        stream,
        &(increment & 0x7fff_ffff).to_be_bytes(),
    )
}

/// Where the next `query` begins its walk. One counter for the whole process:
/// it only decides an offset, so a second endpoint would still be walked
/// correctly, just phase-shifted.
static NEXT_ADDR: AtomicUsize = AtomicUsize::new(0);

/// The order `query` walks `n` addresses in, for a call that drew `start`.
///
/// Split out from `query` because `query` does network I/O and this does not:
/// the property worth pinning is that the walk is still a PERMUTATION of every
/// index вЂ” rotating the start must never cost an address its turn, or a rotation
/// would trade one failure mode for a worse one.
fn walk(n: usize, start: usize) -> impl Iterator<Item = usize> {
    (0..n).map(move |step| start.wrapping_add(step) % n)
}

/// Asks `ep` for `wire`, an RFC 1035 query, and returns the raw reply bytes.
///
/// Every address is tried, within the budget, and the first that answers speaks
/// for the endpoint вЂ” exactly as `ask_provider` treats a UDP provider's address
/// list. What differs is WHERE the walk starts: each call begins one further
/// along, round-robin.
///
/// That rotation is not load-balancing politeness, it is a correctness fix for
/// an assumption this function used to make. An `Endpoint`'s addresses are
/// **peer machines of one service**, not a primary and a backup. Walking them
/// from index 0 every time means the first one that works takes 100 % of the
/// queries and the rest are touched only when it breaks вЂ” which is what
/// happened: measured 2026-09-12, this tool was the entire reason one of
/// dns-ai.ru's two nodes ran at 48 % of a core while the other sat at 15 %
/// (G43). A counter costs nothing, spreads exactly, needs no RNG or new crate,
/// and keeps the fall-through: a dead address still only costs its own slice.
///
/// The rotation and the kept connections do not fight: each address has its
/// own, so alternating between them costs no handshake.
pub fn query(ep: &Endpoint, wire: &[u8], budget: Duration) -> Result<Vec<u8>, String> {
    let answer = query_via(&POOL, ep, wire, budget);
    if answer.is_ok() {
        crate::net::note_reached();
    }
    answer
}

/// `query` over a given pool - separate so a test can count what one pool
/// opened without the rest of the process sharing it.
fn query_via(pool: &Pool, ep: &Endpoint, wire: &[u8], budget: Duration) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + budget;
    let mut last = "РЅРµС‚ Р°РґСЂРµСЃРѕРІ".to_string();
    let n = ep.addrs.len();
    if n == 0 {
        return Err(last);
    }
    let start = NEXT_ADDR.fetch_add(1, Ordering::Relaxed);
    for idx in walk(n, start) {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let addr = ep.addrs[idx];
        let slice = PER_ADDR_BUDGET.min(deadline - now);
        let Ok(ip) = addr.parse::<IpAddr>() else {
            continue;
        };
        match query_one(pool, ep, ip, wire, slice) {
            Ok(reply) => return Ok(reply),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// One address, one query: on a kept connection if there is one, and once
/// more on a fresh one if the kept one turns out to be gone. Both attempts
/// share `budget` - the address's slice - so a retry can never push `query`
/// past its own deadline.
fn query_one(
    pool: &Pool,
    ep: &Endpoint,
    ip: IpAddr,
    wire: &[u8],
    budget: Duration,
) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + budget;
    let path = format!("{}?dns={}", ep.path, base64url(wire));
    let mut kept = pool.take(ep.host, ip);
    loop {
        let reused = kept.is_some();
        let mut conn = match kept.take() {
            Some(conn) => conn,
            None => pool.open(ep, ip, deadline)?,
        };
        let outcome = conn.ask(&path, deadline);
        if keep_after(&outcome, conn.served) {
            pool.put(conn);
        } else {
            pool.dropped.fetch_add(1, Ordering::Relaxed);
            conn.close();
        }
        match outcome {
            Ok(answer) => return Ok(answer.body),
            Err(fail) if retry_on_fresh(reused, &fail) => {
                pool.retried.fetch_add(1, Ordering::Relaxed);
            }
            Err(fail) => return Err(fail.into_message()),
        }
    }
}

/// How an exchange failed, sorted by what may be done about it.
#[derive(Debug)]
enum Fail {
    /// The connection was gone before any of this stream's answer came back:
    /// EOF, a reset, a request that would not go out, or a GOAWAY stopping
    /// short of our stream (RFC 7540 В§6.8: such a stream was not processed).
    /// On a kept connection that is the server having closed it between two
    /// queries - nothing wrong with the address - so the query is asked again
    /// on a fresh one. A DoH GET is idempotent, so asking twice is harmless.
    /// Not a corner case: dns-ai.ru closes kept connections on its own, a clean
    /// close_notify with no GOAWAY after anywhere from 2 to 69 queries, about
    /// one query in twenty on both nodes (measured 2026-09-18).
    Gone(String),
    /// Anything else: a timeout, a reset stream, a malformed frame, an answer
    /// cut off halfway. A second try at the same address would not help;
    /// `query` moves on to the next.
    Broken(String),
}

impl Fail {
    fn into_message(self) -> String {
        match self {
            Fail::Gone(m) | Fail::Broken(m) => m,
        }
    }
}

/// One stream's answer, and whether its connection may carry another.
#[derive(Debug)]
struct Answer {
    body: Vec<u8>,
    /// The server said GOAWAY while this answer was on its way: the answer
    /// stands, the connection is finished.
    last: bool,
}

/// Whether a failed query is asked again on a fresh connection: only when the
/// connection it failed on came from the pool, and only when it failed by
/// being gone. On a fresh connection the same failure is the address failing,
/// and `query` walks on exactly as it did before connections were kept.
fn retry_on_fresh(reused: bool, fail: &Fail) -> bool {
    reused && matches!(fail, Fail::Gone(_))
}

/// Whether a connection goes back to the pool after an exchange: only one that
/// read its answer cleanly, that the server has not said GOAWAY on, and that
/// has requests left. After any error its stream state is unknown - a frame
/// half read, a stream half answered - and the next query would inherit that.
fn keep_after(outcome: &Result<Answer, Fail>, served: u32) -> bool {
    matches!(outcome, Ok(answer) if !answer.last) && served < MAX_REQUESTS
}

/// Whether an idle connection is still worth taking: used for fewer than
/// `MAX_REQUESTS` queries and idle for less than `IDLE_LIMIT`. `now` is a
/// parameter so the rule can be tested without waiting 20 s.
fn takeable(last_used: Instant, served: u32, now: Instant) -> bool {
    served < MAX_REQUESTS && now.saturating_duration_since(last_used) < IDLE_LIMIT
}

/// The process's kept connections.
static POOL: Pool = Pool::new();

/// Idle h2 connections, per address. A connection is in here only while
/// nobody is using it: `take` removes it, `put` returns it, so two threads can
/// never hold the same one. A thread that finds none opens its own rather than
/// waiting for another thread to finish with one.
struct Pool {
    idle: Mutex<Vec<Conn>>,
    /// Connections this pool opened, how many of them resumed a TLS session,
    /// how many were closed after a query instead of kept (an error, a GOAWAY,
    /// the request limit), and how many queries found their kept connection
    /// already closed by the server. Read by the live tests; the relay does
    /// not report them.
    opened: AtomicUsize,
    resumed: AtomicUsize,
    dropped: AtomicUsize,
    retried: AtomicUsize,
}

impl Pool {
    const fn new() -> Pool {
        Pool {
            idle: Mutex::new(Vec::new()),
            opened: AtomicUsize::new(0),
            resumed: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
            retried: AtomicUsize::new(0),
        }
    }

    /// The most recently used idle connection to `ip`, if one is still worth
    /// using. Anything too old, to any address, is closed on the way: the pool
    /// has no thread of its own, so a lookup is when it gets tidied.
    fn take(&self, host: &str, ip: IpAddr) -> Option<Conn> {
        let now = Instant::now();
        let (found, stale) = {
            let Ok(mut idle) = self.idle.lock() else {
                return None;
            };
            let (fresh, stale): (Vec<Conn>, Vec<Conn>) = std::mem::take(&mut *idle)
                .into_iter()
                .partition(|c| takeable(c.last_used, c.served, now));
            *idle = fresh;
            let found = idle
                .iter()
                .rposition(|c| c.ip == ip && c.host == host)
                .map(|i| idle.remove(i));
            (found, stale)
        };
        // Outside the lock: a close writes to a socket.
        for conn in stale {
            conn.close();
        }
        found
    }

    /// Returns a connection for the next query. At most `MAX_IDLE_PER_ADDR` are
    /// kept per address; past that the longest-idle one is closed.
    fn put(&self, conn: Conn) {
        let evicted = {
            let Ok(mut idle) = self.idle.lock() else {
                conn.close();
                return;
            };
            let (ip, host) = (conn.ip, conn.host);
            let same = |c: &Conn| c.ip == ip && c.host == host;
            let evicted = if idle.iter().filter(|c| same(c)).count() >= MAX_IDLE_PER_ADDR {
                idle.iter().position(same).map(|i| idle.remove(i))
            } else {
                None
            };
            idle.push(conn);
            evicted
        };
        if let Some(conn) = evicted {
            conn.close();
        }
    }

    fn open(&self, ep: &Endpoint, ip: IpAddr, deadline: Instant) -> Result<Conn, String> {
        let conn = Conn::open(ep, ip, deadline)?;
        self.opened.fetch_add(1, Ordering::Relaxed);
        if conn.tls.handshake_kind() == Some(HandshakeKind::Resumed) {
            self.resumed.fetch_add(1, Ordering::Relaxed);
        }
        Ok(conn)
    }

    /// Closes every idle connection. For the live tests only: the next query
    /// has to open - and, with its address's session store, resume.
    #[cfg(test)]
    fn clear(&self) {
        let all = match self.idle.lock() {
            Ok(mut idle) => std::mem::take(&mut *idle),
            Err(_) => return,
        };
        for conn in all {
            conn.close();
        }
    }
}

/// The socket under a kept connection: every read and every write is bounded
/// by one deadline, not by a timeout set once per query.
///
/// A timeout bounds a single `recv`, and one `rustls::Stream::read` can make
/// several - it loops until it holds a whole TLS record, taking in any session
/// ticket on the way - so an answer that trickled in could take the timeout
/// several times over and overrun the slice a retry has to fit in. Re-deriving
/// it from the deadline before every syscall is what makes the budget a budget.
struct Bounded {
    sock: TcpStream,
    deadline: Instant,
}

impl Bounded {
    fn left(&self) -> std::io::Result<Duration> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            Err(ErrorKind::TimedOut.into())
        } else {
            Ok(left)
        }
    }
}

impl Read for Bounded {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.sock.set_read_timeout(Some(self.left()?))?;
        self.sock.read(buf)
    }
}

impl Write for Bounded {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.sock.set_write_timeout(Some(self.left()?))?;
        self.sock.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.sock.flush()
    }
}

/// One h2 connection to one address, kept between queries.
struct Conn {
    host: &'static str,
    ip: IpAddr,
    sock: Bounded,
    tls: ClientConnection,
    /// Sent ahead of the first request and never again: the preface, our
    /// SETTINGS and the connection WINDOW_UPDATE. Queued rather than written at
    /// open so the first request still leaves in one write, as it always has.
    unsent: Vec<u8>,
    /// Client streams are odd and strictly increasing (RFC 7540 В§5.1.1).
    next_stream: u32,
    /// Frame bytes read but not yet parsed, carried between queries - see
    /// `read_reply`.
    buf: Vec<u8>,
    last_used: Instant,
    /// Requests this connection has carried, `MAX_REQUESTS` at most.
    served: u32,
}

impl Conn {
    fn open(ep: &Endpoint, ip: IpAddr, deadline: Instant) -> Result<Conn, String> {
        let (sock, tls) = handshake(ep, ip, deadline)?;
        let mut unsent = Vec::with_capacity(64);
        unsent.extend_from_slice(PREFACE);
        unsent.extend_from_slice(&frame(FRAME_SETTINGS, 0, 0, &[]));
        unsent.extend_from_slice(&window_update(0, CONN_WINDOW_INCREMENT));
        Ok(Conn {
            host: ep.host,
            ip,
            sock: Bounded { sock, deadline },
            tls,
            unsent,
            next_stream: 1,
            buf: Vec::with_capacity(4096),
            last_used: Instant::now(),
            served: 0,
        })
    }

    /// One request on the next stream, and its answer.
    fn ask(&mut self, path: &str, deadline: Instant) -> Result<Answer, Fail> {
        let ip = self.ip;
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Fail::Broken(format!("{}: Р±СЋРґР¶РµС‚ РёСЃС‚С‘Рє РґРѕ Р·Р°РїСЂРѕСЃР°", ip)));
        }
        // A kept socket still carries the deadline of the query before it.
        self.sock.deadline = deadline;

        let stream = self.next_stream;
        self.next_stream += 2;
        self.served += 1;
        let mut out = std::mem::take(&mut self.unsent);
        out.extend_from_slice(&frame(
            FRAME_HEADERS,
            FLAG_END_HEADERS | FLAG_END_STREAM,
            stream,
            &request_headers(self.host, path),
        ));

        let mut tls = rustls::Stream::new(&mut self.tls, &mut self.sock);
        // Disambiguated: `rustls::Stream` offers these names through both
        // `std::io::Write` and `ReadWrite`. The flush is what reports a socket
        // that is already closed - `write` swallows that error by design.
        let sent =
            std::io::Write::write_all(&mut tls, &out).and_then(|_| std::io::Write::flush(&mut tls));
        let answer = match sent {
            Ok(()) => read_reply(&mut tls, &mut self.buf, stream, ip, deadline),
            Err(_) => Err(Fail::Gone(format!("{}: Р·Р°РїСЂРѕСЃ РЅРµ СѓС€С‘Р»", ip))),
        };
        self.last_used = Instant::now();
        answer
    }

    /// Ends the connection politely - close_notify, so the server logs a clean
    /// close rather than a reset - without letting that cost anything: errors
    /// are ignored and the write gets a short leash. The socket closes on drop.
    fn close(mut self) {
        self.sock.deadline = Instant::now() + Duration::from_millis(200);
        self.tls.send_close_notify();
        let _ = self.tls.write_tls(&mut self.sock);
    }
}

/// TCP + TLS to one address, h2 negotiated, all inside `deadline`.
fn handshake(
    ep: &Endpoint,
    ip: IpAddr,
    deadline: Instant,
) -> Result<(TcpStream, ClientConnection), String> {
    let remaining = |d: Instant| d.saturating_duration_since(Instant::now());
    let budget = remaining(deadline);
    if budget.is_zero() {
        return Err(format!("{}: Р±СЋРґР¶РµС‚ РёСЃС‚С‘Рє РґРѕ СЃРѕРµРґРёРЅРµРЅРёСЏ", ip));
    }
    // Pinned to the ISP link while a tunnel holds the default route (P13): a
    // provider that substitutes only for Russian addresses answers a query that
    // arrives from the tunnel's foreign exit with the genuine address.
    let mut sock = crate::net::connect(
        SocketAddr::new(ip, 443),
        budget,
        crate::net::pin_interface(),
    )
    .map_err(|_| format!("{}: РЅРµС‚ СЃРѕРµРґРёРЅРµРЅРёСЏ", ip))?;
    sock.set_nodelay(true).ok();

    let server = ServerName::try_from(ep.host.to_string())
        .map_err(|_| "РЅРµРІРµСЂРЅРѕРµ РёРјСЏ DoH-СЃРµСЂРІРµСЂР°".to_string())?;
    let mut conn = ClientConnection::new(tls_config(ip), server)
        .map_err(|e| format!("TLS РЅРµ РЅР°СЃС‚СЂРѕРµРЅ: {}", e))?;

    // Driven by hand so the budget covers the whole handshake rather than each
    // syscall inside it - the same shape `resolvers::reachable` uses, and for
    // the same reason: an address that completes TCP and stalls in TLS is the
    // failure this has to notice quickly (G23).
    while conn.is_handshaking() {
        let left = remaining(deadline);
        if left.is_zero() {
            return Err(format!("{}: TLS РЅРµ СѓР»РѕР¶РёР»СЃСЏ РІ Р±СЋРґР¶РµС‚", ip));
        }
        sock.set_read_timeout(Some(left)).ok();
        sock.set_write_timeout(Some(left)).ok();
        if conn.wants_write() {
            conn.write_tls(&mut sock)
                .map_err(|_| format!("{}: РѕР±СЂС‹РІ РїСЂРё handshake", ip))?;
        }
        if conn.is_handshaking() && conn.wants_read() {
            match conn.read_tls(&mut sock) {
                Ok(0) => return Err(format!("{}: СЃРµСЂРІРµСЂ Р·Р°РєСЂС‹Р» handshake", ip)),
                Ok(_) => conn
                    .process_new_packets()
                    .map(|_| ())
                    .map_err(|e| format!("{}: TLS РѕС‚РєР»РѕРЅС‘РЅ: {}", ip, e))?,
                Err(_) => return Err(format!("{}: РѕР±СЂС‹РІ РїСЂРё handshake", ip)),
            }
        }
    }
    if conn.alpn_protocol() != Some(b"h2") {
        return Err(format!("{}: СЃРµСЂРІРµСЂ РЅРµ СЃРѕРіР»Р°СЃРѕРІР°Р» h2", ip));
    }
    Ok((sock, conn))
}

/// Reads frames until `stream` ends, collecting its DATA.
///
/// `buf` belongs to the connection, not to this call. One `read` can return the
/// end of this answer together with whatever the server sent next - a PING, a
/// SETTINGS, a WINDOW_UPDATE - and a buffer local to the call dropped those
/// bytes, so the next query on the connection would start parsing in the middle
/// of a frame. Frames already in `buf` are parsed before anything is read, and
/// whatever follows the end of this stream is left there for the next call.
fn read_reply(
    tls: &mut impl ReadWrite,
    buf: &mut Vec<u8>,
    stream: u32,
    ip: IpAddr,
    deadline: Instant,
) -> Result<Answer, Fail> {
    let mut body: Vec<u8> = Vec::new();
    // Whether any frame of THIS stream's answer has arrived. Until one has, a
    // connection that dies has told us nothing about the query (`Fail::Gone`).
    let mut started = false;
    let mut goaway = false;
    let mut chunk = [0u8; 4096];
    let lost = |started: bool, why: String| {
        if started {
            Fail::Broken(why)
        } else {
            Fail::Gone(why)
        }
    };

    loop {
        while buf.len() >= 9 {
            let len = u32::from_be_bytes([0, buf[0], buf[1], buf[2]]) as usize;
            if len > MAX_BODY {
                return Err(Fail::Broken(format!(
                    "{}: РєР°РґСЂ РґР»РёРЅРѕР№ {} вЂ” РЅРµ РѕС‚РІРµС‚ DNS",
                    ip, len
                )));
            }
            if buf.len() < 9 + len {
                break;
            }
            let kind = buf[3];
            let flags = buf[4];
            let id = u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]]) & 0x7fff_ffff;
            let payload: Vec<u8> = buf[9..9 + len].to_vec();
            buf.drain(..9 + len);
            let word = |at: usize| {
                payload
                    .get(at..at + 4)
                    .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
                    .unwrap_or(0)
            };

            match kind {
                FRAME_SETTINGS if flags & FLAG_ACK == 0 => {
                    tls.write_all(&frame(FRAME_SETTINGS, FLAG_ACK, 0, &[])).ok();
                    tls.flush().ok();
                }
                FRAME_PING if flags & FLAG_ACK == 0 => {
                    tls.write_all(&frame(FRAME_PING, FLAG_ACK, 0, &payload))
                        .ok();
                    tls.flush().ok();
                }
                FRAME_DATA if id == stream => {
                    started = true;
                    body.extend_from_slice(&payload);
                    if body.len() > MAX_BODY {
                        return Err(Fail::Broken(format!("{}: РѕС‚РІРµС‚ СЃР»РёС€РєРѕРј РІРµР»РёРє", ip)));
                    }
                    if flags & FLAG_END_STREAM != 0 {
                        return Ok(Answer { body, last: goaway });
                    }
                }
                FRAME_HEADERS if id == stream => {
                    started = true;
                    // A response with no body: nothing to hand back, and saying
                    // so beats returning bytes that will fail to parse as DNS.
                    if flags & FLAG_END_STREAM != 0 {
                        return if body.is_empty() {
                            Err(Fail::Broken(format!("{}: РѕС‚РІРµС‚ Р±РµР· С‚РµР»Р°", ip)))
                        } else {
                            Ok(Answer { body, last: goaway })
                        };
                    }
                }
                FRAME_RST_STREAM if id == stream => {
                    return Err(Fail::Broken(format!(
                        "{}: РїРѕС‚РѕРє СЃР±СЂРѕС€РµРЅ, РєРѕРґ {}",
                        ip,
                        word(0)
                    )));
                }
                FRAME_GOAWAY => {
                    let last_id = word(0) & 0x7fff_ffff;
                    if last_id < stream {
                        return Err(Fail::Gone(format!("{}: GOAWAY, РєРѕРґ {}", ip, word(4))));
                    }
                    // Our stream is still answered; nothing after it will be.
                    goaway = true;
                }
                _ => {}
            }
        }

        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Fail::Broken(format!("{}: DoH РЅРµ РѕС‚РІРµС‚РёР» РІ СЃСЂРѕРє", ip)));
        }
        let n = match tls.read(&mut chunk) {
            Ok(n) => n,
            Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                return Err(Fail::Broken(format!("{}: DoH РЅРµ РѕС‚РІРµС‚РёР» РІ СЃСЂРѕРє", ip)));
            }
            Err(_) => return Err(lost(started, format!("{}: РѕР±СЂС‹РІ РѕС‚РІРµС‚Р°", ip))),
        };
        // An end before END_STREAM is a cut, not an answer - even with some
        // DATA in hand, those bytes are a truncated DNS message.
        if n == 0 {
            return Err(lost(started, format!("{}: СЃРµСЂРІРµСЂ Р·Р°РєСЂС‹Р» СЃРѕРµРґРёРЅРµРЅРёРµ", ip)));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// What `read_reply` needs from a connection; a trait only so it can be
/// exercised without a socket. The deadline is not in it: the socket under
/// the real one enforces it on every syscall (`Bounded`).
trait ReadWrite {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()>;
    fn flush(&mut self) -> std::io::Result<()>;
}

impl ReadWrite for rustls::Stream<'_, ClientConnection, Bounded> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        Read::read(self, buf)
    }
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        Write::write_all(self, buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Write::flush(self)
    }
}

#[cfg(test)]
mod tests;

//! Why a local port could not be bound, in words a user can act on.
//!
//! The relay listens on three loopback ports: the local proxy's TCP port, the
//! gate hosts' door on TCP `:443`, and the DNS relay's UDP `127.0.0.53:53`.
//! A UDP `:53` bind cannot be moved to another port (the NRPT rules that point
//! at the relay carry an IP address with no port), so for that listener the
//! diagnosis is the whole of what we can offer the user. When a bind fails the
//! relay used to log `os error 10013` and carry on without them, and Antigravity
//! went on hitting the 400 with nothing on screen saying why (P53). Three causes
//! cover what the field has shown, and each has a different fix:
//!
//! * **held** — another program already listens on that port (a local web
//!   server, VMware's shared VMs, Docker, IIS through HTTP.sys; on UDP `:53` the
//!   usual holder is Windows' Internet Connection Sharing (the `SharedAccess`
//!   service, which the Hyper-V default switch and the mobile hotspot both
//!   start), a local DNS proxy (AdGuard, DNSCrypt, Docker Desktop) or a
//!   competing tool). We can name it.
//! * **reserved** — the port lies in a range Windows has set aside, which is
//!   what Hyper-V, WSL 2 and Docker do inside the dynamic range (G31).
//! * **denied** — nothing holds it and nothing reserved it, and Windows still
//!   says `WSAEACCES`: security software refusing this program a listening
//!   socket. The fix is an exception for our exe in that software.

use std::net::{Ipv4Addr, SocketAddr};

/// Which transport a failed bind was on. The listener tables and Windows'
/// excluded-port ranges are kept per protocol, so a diagnosis has to ask about
/// the right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    Tcp,
    Udp,
}

/// The program listening where a bind failed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Holder {
    /// Its image name, when it could be read (a service run as SYSTEM may not
    /// tell an unelevated relay).
    pub by: Option<String>,
    /// The process id, so the window can end exactly the process it was told
    /// about rather than whoever holds the port by the time it acts.
    pub pid: u32,
    /// Whether this is a leftover copy of our own relay - the one holder the
    /// window may offer to end, because it is ours and the relay that wants
    /// the port back is already retrying every minute.
    pub ours: bool,
}

/// What stopped a bind, as `gate::Blocker::cause` spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cause {
    /// Another program listens on the port.
    Held(Holder),
    /// Inside a range Windows excluded from use.
    Reserved,
    /// Access denied with no holder and no reservation: security software.
    Denied,
    /// Anything else; the OS text says what.
    Other,
}

impl Cause {
    pub fn code(&self) -> &'static str {
        match self {
            Cause::Held(_) => "held",
            Cause::Reserved => "reserved",
            Cause::Denied => "denied",
            Cause::Other => "other",
        }
    }

    pub fn holder(&self) -> Option<&Holder> {
        match self {
            Cause::Held(h) => Some(h),
            _ => None,
        }
    }

    /// The record the relay publishes for the window (`gate::set_blocker`).
    pub fn blocker(
        &self,
        what: &str,
        addr: SocketAddr,
        err: &std::io::Error,
    ) -> crate::gate::Blocker {
        let holder = self.holder();
        crate::gate::Blocker {
            what: what.to_string(),
            addr: addr.to_string(),
            cause: self.code().to_string(),
            by: holder
                .and_then(|h| h.by.clone())
                .unwrap_or_default(),
            pid: holder.map_or(0, |h| h.pid),
            ours: holder.is_some_and(|h| h.ours),
            error: err.to_string(),
        }
    }

    /// Whether moving to another port gets around it. Security software that
    /// refuses this program a socket refuses it every port.
    ///
    /// Windows only. On Linux nothing would carry a moved port into the
    /// `environment.d` drop-in (the relay's env thread is Windows'), so a move
    /// would leave the variable naming the old port; there the proxy keeps
    /// retrying its own port instead, which a borrowed ephemeral port frees.
    pub fn another_port_helps(&self) -> bool {
        cfg!(windows) && !matches!(self, Cause::Denied)
    }
}

/// Classifies a failed bind of `addr` on TCP.
pub fn diagnose(addr: SocketAddr, err: &std::io::Error) -> Cause {
    diagnose_on(addr, err, Proto::Tcp)
}

/// Classifies a failed bind of `addr` on `proto`. Costs a table read and, only
/// when that finds no holder, one `netsh` call: once per failed bind, which
/// while a blocker lasts is the listener's one retry a minute.
pub fn diagnose_on(addr: SocketAddr, err: &std::io::Error, proto: Proto) -> Cause {
    if let Some(holder) = listener_holder(addr, proto) {
        return Cause::Held(holder);
    }
    if excluded_ranges(proto)
        .iter()
        .any(|(lo, hi)| (*lo..=*hi).contains(&addr.port()))
    {
        return Cause::Reserved;
    }
    // 10013 on Windows, EACCES elsewhere.
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        return Cause::Denied;
    }
    Cause::Other
}

// ---------------------------------------------------------------------------
// Who is listening
// ---------------------------------------------------------------------------

/// The holder of `addr` to name: the exact address first, the wildcard only
/// after it, and a copy of our own relay ahead of anyone else.
///
/// Both listener tables come back in an order of Windows' choosing and one port
/// can have more than one holder - Internet Connection Sharing keeps
/// `0.0.0.0:53` while a relay of ours keeps `127.0.0.53:53`, and a stale relay
/// next to a fresh one holds the same addresses twice over. Taking the first
/// row that matched sent the user after `svchost.exe` and a service they cannot
/// stop, and offered them a button that cannot free the port, while the copy
/// that actually had to go sat in the same table (P53, measured on a machine
/// where exactly this happened).
fn listener_holder(addr: SocketAddr, proto: Proto) -> Option<Holder> {
    let mut best: Option<Holder> = None;
    for pid in listener_pids(addr, proto) {
        let by = process_name(pid);
        let holder = Holder {
            ours: is_our_relay(pid, by.as_deref()),
            by,
            pid,
        };
        // The list is already exact-address first, so the first candidate is
        // the one that actually collides with the bind; a copy of ours moves
        // ahead of it only because it is also the one the window can act on.
        let take = match &best {
            None => true,
            Some(kept) => holder.ours && !kept.ours,
        };
        if take {
            best = Some(holder);
        }
    }
    best
}

/// The order candidates are offered in: the address asked for, then the
/// wildcard, one entry per process.
///
/// The split is what makes the answer right rather than merely plausible: a
/// bind for `127.0.0.53:53` collides with whatever holds *that* address, and
/// Windows lets it sit next to a `0.0.0.0:53` (measured - ICS on a machine
/// with a working relay on it). A wildcard row is a real blocker only when
/// there is no exact one, which is the case the advice card is written for.
fn ordered_holders(rows: &[(Ipv4Addr, u32)], want: Ipv4Addr) -> Vec<u32> {
    let mut exact = Vec::new();
    let mut wildcard = Vec::new();
    for (local, pid) in rows {
        if *local == want {
            exact.push(*pid);
        } else if local.is_unspecified() {
            wildcard.push(*pid);
        }
    }
    exact.extend(wildcard);
    exact.dedup();
    exact
}

/// The image name of the program listening where `addr` points, when it can be
/// read. What tells our own proxy from somebody else's program answering on the
/// same port (`proxy::wait_for_our_listener`).
pub fn listener_image(addr: SocketAddr) -> Option<String> {
    listener_holder(addr, Proto::Tcp).and_then(|h| h.by)
}

/// Every TCP listener on `addr`'s port, with the address each one holds.
///
/// The whole port is read rather than only the wanted address because the
/// wildcard has to be ranked against the exact one, and a listener on another
/// loopback address with the same port is neither of those - it does not stand
/// in the way, and blaming it would send the user after the wrong program (a
/// dev server on 127.0.0.1:443 while the door wants 127.65.71.1:443).
#[cfg(windows)]
fn tcp_rows(addr: SocketAddr) -> Vec<(Ipv4Addr, u32)> {
    let port = match addr {
        SocketAddr::V4(a) => a.port(),
        SocketAddr::V6(_) => return Vec::new(),
    };
    use std::ffi::c_void;
    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedTcpTable(
            table: *mut c_void,
            size: *mut u32,
            order: i32,
            af: u32,
            class: u32,
            reserved: u32,
        ) -> u32;
    }
    const AF_INET: u32 = 2;
    const TCP_TABLE_OWNER_PID_LISTENER: u32 = 3;
    const NO_ERROR: u32 = 0;
    // MIB_TCPROW_OWNER_PID: state, local addr, local port, remote addr, remote
    // port, pid - six u32s, after the table's one-u32 count.
    const ROW: usize = 6;

    let mut size = 0u32;
    unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
    }
    // A little slack: the table can grow between the two calls.
    let mut buf = vec![0u32; (size as usize / 4) + 64];
    let mut size = (buf.len() * 4) as u32;
    let rc = unsafe {
        GetExtendedTcpTable(
            buf.as_mut_ptr() as *mut c_void,
            &mut size,
            0,
            AF_INET,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if rc != NO_ERROR {
        return Vec::new();
    }
    let count = (buf[0] as usize).min((buf.len() - 1) / ROW);
    (0..count)
        .filter_map(|i| {
            let row = &buf[1 + i * ROW..1 + (i + 1) * ROW];
            // The port sits in the low 16 bits, in network byte order; the address
            // is an `in_addr`, i.e. its bytes are already in network order.
            let local_port = u16::from_be((row[2] & 0xFFFF) as u16);
            (local_port == port).then(|| (Ipv4Addr::from(row[1].to_ne_bytes()), row[5]))
        })
        .collect()
}

/// Every bound UDP socket on `addr`'s port, with the address each one holds.
/// Ranked by `ordered_holders`, so an exclusive `0.0.0.0:53` (Windows'
/// Internet Connection Sharing does exactly this) is named only when nothing
/// holds the address actually asked for.
#[cfg(windows)]
fn udp_rows(addr: SocketAddr) -> Vec<(Ipv4Addr, u32)> {
    let port = match addr {
        SocketAddr::V4(a) => a.port(),
        SocketAddr::V6(_) => return Vec::new(),
    };
    use std::ffi::c_void;
    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedUdpTable(
            table: *mut c_void,
            size: *mut u32,
            order: i32,
            af: u32,
            class: u32,
            reserved: u32,
        ) -> u32;
    }
    const AF_INET: u32 = 2;
    const UDP_TABLE_OWNER_PID: u32 = 1;
    const NO_ERROR: u32 = 0;
    // MIB_UDPROW_OWNER_PID: dwLocalAddr, dwLocalPort, dwOwningPid -
    // three u32s, after the table's one-u32 count.
    const ROW: usize = 3;

    let mut size = 0u32;
    unsafe {
        GetExtendedUdpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET,
            UDP_TABLE_OWNER_PID,
            0,
        );
    }
    // A little slack: the table can grow between the two calls.
    let mut buf = vec![0u32; (size as usize / 4) + 64];
    let mut size = (buf.len() * 4) as u32;
    let rc = unsafe {
        GetExtendedUdpTable(
            buf.as_mut_ptr() as *mut c_void,
            &mut size,
            0,
            AF_INET,
            UDP_TABLE_OWNER_PID,
            0,
        )
    };
    if rc != NO_ERROR {
        return Vec::new();
    }
    let count = (buf[0] as usize).min((buf.len() - 1) / ROW);
    // Note: UDP has no LISTEN state, so every bound UDP socket is in this table.
    (0..count)
        .filter_map(|i| {
            let row = &buf[1 + i * ROW..1 + (i + 1) * ROW];
            let local_port = u16::from_be((row[1] & 0xFFFF) as u16);
            (local_port == port).then(|| (Ipv4Addr::from(row[0].to_ne_bytes()), row[2]))
        })
        .collect()
}

/// The pids holding `addr`, in the order `listener_holder` wants them.
#[cfg(windows)]
fn listener_pids(addr: SocketAddr, proto: Proto) -> Vec<u32> {
    let want = match addr {
        SocketAddr::V4(a) => *a.ip(),
        SocketAddr::V6(_) => return Vec::new(),
    };
    let rows = match proto {
        Proto::Tcp => tcp_rows(addr),
        Proto::Udp => udp_rows(addr),
    };
    ordered_holders(&rows, want)
}

/// The image name of `pid`: the kernel's answer when this process may open the
/// other one, `tasklist`'s otherwise (it reads names across sessions and
/// privilege levels without elevation).
#[cfg(windows)]
fn process_name(pid: u32) -> Option<String> {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn QueryFullProcessImageNameW(
            process: *mut c_void,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    // PID 4 is the kernel: a listener it owns is HTTP.sys, i.e. IIS or a
    // service that registered a URL with it. Its name would say nothing.
    if pid == 4 {
        return Some("служба Windows HTTP (IIS или похожая)".to_string());
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if !h.is_null() {
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0;
            CloseHandle(h);
            if ok {
                let full = String::from_utf16_lossy(&buf[..len as usize]);
                return full.rsplit('\\').next().map(str::to_string);
            }
        }
    }
    let mut cmd = std::process::Command::new("tasklist");
    cmd.args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]);
    let out = crate::utils::bounded_output(
        crate::utils::no_window(&mut cmd),
        std::time::Duration::from_secs(5),
    )?;
    let text = String::from_utf8_lossy(&out.stdout);
    let name = text
        .lines()
        .next()?
        .split(',')
        .next()?
        .trim_matches('"')
        .trim();
    // An image name, not tasklist's "INFO: No tasks are running…" for a PID
    // that exited between the two reads (that line has a dot too).
    name.to_ascii_lowercase()
        .ends_with(".exe")
        .then(|| name.to_string())
}

/// Whether an image name could be this tool's relay at all: the installed
/// `OpenAntigravityRelay.exe`, or this very exe (the portable relay is the window's own
/// image).
fn is_our_relay_image(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    if name.is_empty() {
        return false;
    }
    if name == crate::platform::service::RELAY_IMAGE {
        return true;
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_ascii_lowercase()))
        .is_some_and(|ours| ours == name)
}

/// Whether the holder is a relay of ours, and so the one the window may offer
/// to end.
///
/// The command line is what separates the portable relay from the window they
/// are both, and it has to be read: offering to end a process that is the
/// window the user is reading the card in would take the whole interface down
/// with the bypass. Measured on the machine this was written for - a stale
/// installed relay and the portable one side by side, each ready to be told it
/// was the other.
#[cfg(windows)]
fn is_our_relay(pid: u32, name: Option<&str>) -> bool {
    let Some(name) = name else { return false };
    if !is_our_relay_image(name) {
        return false;
    }
    // The installed image is the relay by construction - no other program of
    // ours is ever called that, and the window never is.
    if name.eq_ignore_ascii_case(crate::platform::service::RELAY_IMAGE) {
        return true;
    }
    let mut cmd = std::process::Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &format!(
            "(Get-CimInstance Win32_Process -Filter \"ProcessId = {pid}\" -ErrorAction \
             SilentlyContinue).CommandLine"
        ),
    ]);
    crate::utils::bounded_output(
        crate::utils::no_window(&mut cmd),
        std::time::Duration::from_secs(5),
    )
    .is_some_and(|o| String::from_utf8_lossy(&o.stdout).contains(crate::platform::service::FORWARDER_FLAG))
}

/// Nothing on this platform has ever answered as our relay: the port layer
/// there is a proxy port, and no field report has asked for a name.
#[cfg(not(windows))]
fn is_our_relay(_pid: u32, _name: Option<&str>) -> bool {
    false
}

/// The port ranges Windows keeps from use for `proto`.
#[cfg(windows)]
fn excluded_ranges(proto: Proto) -> Vec<(u16, u16)> {
    let proto_arg = match proto {
        Proto::Tcp => "protocol=tcp",
        Proto::Udp => "protocol=udp",
    };
    let mut cmd = std::process::Command::new("netsh");
    cmd.args(["int", "ipv4", "show", "excludedportrange", proto_arg]);
    crate::utils::bounded_output(
        crate::utils::no_window(&mut cmd),
        std::time::Duration::from_secs(5),
    )
    .map(|out| parse_excluded_ranges(&String::from_utf8_lossy(&out.stdout)))
    .unwrap_or_default()
}

/// `netsh`'s table, in whatever language Windows speaks: every line that opens
/// with two port numbers is a range, and nothing else in its output does.
fn parse_excluded_ranges(text: &str) -> Vec<(u16, u16)> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let lo = parts.next()?.parse::<u16>().ok()?;
            let hi = parts.next()?.parse::<u16>().ok()?;
            (lo <= hi).then_some((lo, hi))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Elsewhere: no reservations, and a holder would need /proc walking that no
// field report has asked for yet.
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
fn listener_pids(_addr: SocketAddr, _proto: Proto) -> Vec<u32> {
    Vec::new()
}

#[cfg(not(windows))]
fn process_name(_pid: u32) -> Option<String> {
    None
}

#[cfg(not(windows))]
fn excluded_ranges(_proto: Proto) -> Vec<(u16, u16)> {
    Vec::new()
}

/// Ends the process the card named, after checking it is still that one.
///
/// The pid comes from the record rather than from a fresh table read: the
/// window shows the user a name, and by the time the button is pressed the
/// port may be held by a different program entirely - killing that one instead
/// would be ending somebody else's software on a guess. `expected` is the
/// image the card showed, and a mismatch is refused rather than acted on.
#[cfg(windows)]
pub fn kill_holding_process(pid: u32, expected: &str) -> Result<u32, String> {
    use std::process::Command;

    if pid == 0 {
        return Err("в записи не было номера процесса".to_string());
    }
    let now = process_name(pid).unwrap_or_default().to_ascii_lowercase();
    let said = expected.trim().to_ascii_lowercase();
    if !said.is_empty() && !now.is_empty() && now != said {
        return Err(format!(
            "порт уже занят другой программой («{now}» вместо «{said}») — закрывать её не будем"
        ));
    }
    let mut cmd = Command::new("taskkill");
    cmd.args(["/F", "/PID", &pid.to_string()]);
    crate::utils::no_window(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| format!("taskkill не запустился: {e}"))?;
    if out.status.success() {
        Ok(pid)
    } else {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if why.is_empty() {
            format!("taskkill ответил кодом {}", out.status.code().unwrap_or(-1))
        } else {
            why
        })
    }
}

/// Nothing to end with on non-Windows from here: the Linux port's DNS layer
/// is stubbed, and its proxy port holders keep the advice-only card.
#[cfg(not(windows))]
pub fn kill_holding_process(_pid: u32, _expected: &str) -> Result<u32, String> {
    Err("не поддерживается на этой платформе".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netsh_ranges_are_read_in_any_language_and_nothing_else_is() {
        let english = "\nProtocol tcp Port Exclusion Ranges\n\nStart Port    End Port\n\
                       ----------    --------\n      5357        5357\n     50000       50059     *\n\
                       \n* - Administered port exclusions.\n";
        assert_eq!(
            parse_excluded_ranges(english),
            vec![(5357, 5357), (50000, 50059)]
        );
        let russian = "\nДиапазоны исключенных портов для протокола tcp\n\n\
                       Начальный порт    Конечный порт\n--------------    -------------\n\
                             53100            53199\n";
        assert_eq!(parse_excluded_ranges(russian), vec![(53100, 53199)]);
        assert!(parse_excluded_ranges("").is_empty());
        assert!(parse_excluded_ranges("  70000 70001\n  9 3\n").is_empty());
    }

    /// The exact address is the blocker; a wildcard beside it is not. Measured
    /// on a real machine: ICS holding `0.0.0.0:53` while the relay held
    /// `127.0.0.53:53`, and the first row that matched was ICS' - which put
    /// `svchost.exe` and a service the user cannot stop on the card, and left
    /// the stale relay that actually held the port unnamed.
    #[test]
    fn the_exact_address_outranks_the_wildcard_beside_it() {
        let want = Ipv4Addr::new(127, 0, 0, 53);
        let both = [
            (Ipv4Addr::UNSPECIFIED, 4),
            (want, 11928),
            (Ipv4Addr::new(127, 65, 71, 1), 900),
        ];
        assert_eq!(ordered_holders(&both, want), vec![11928, 4]);
        // Alone on the port, the wildcard is the only answer there is - that is
        // the ICS case the advice card is written for.
        assert_eq!(
            ordered_holders(&[(Ipv4Addr::UNSPECIFIED, 4)], want),
            vec![4]
        );
        // A listener on another address of the same port is in nobody's way.
        let elsewhere = [(Ipv4Addr::new(127, 65, 71, 1), 900)];
        assert!(ordered_holders(&elsewhere, want).is_empty());
    }

    /// Live: a port this test holds is found, with this test's own name.
    #[test]
    #[cfg(windows)]
    fn a_port_we_listen_on_is_held_by_us() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let here = l.local_addr().unwrap();
        let holder = listener_holder(here, Proto::Tcp).expect("our own listener in the table");
        assert_eq!(holder.pid, std::process::id());
        let name = holder.by.unwrap_or_default().to_ascii_lowercase();
        assert!(name.ends_with(".exe"), "{name}");
        // The same port on another loopback address is not in its way.
        let elsewhere = SocketAddr::from(([127, 65, 71, 9], here.port()));
        assert!(listener_holder(elsewhere, Proto::Tcp).is_none());
    }

    #[test]
    #[cfg(windows)]
    fn a_udp_port_we_hold_is_found() {
        let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let here = s.local_addr().unwrap();
        let holder = listener_holder(here, Proto::Udp).expect("our own UDP socket in the table");
        assert_eq!(holder.pid, std::process::id());
        let name = holder.by.unwrap_or_default().to_ascii_lowercase();
        assert!(name.ends_with(".exe"), "{name}");
        // The same port on another loopback address is not matched.
        let elsewhere = SocketAddr::from(([127, 65, 71, 9], here.port()));
        assert!(listener_holder(elsewhere, Proto::Udp).is_none());
    }

    #[test]
    #[cfg(windows)]
    fn tcp_and_udp_tables_are_asked_separately() {
        let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let tcp_addr = tcp.local_addr().unwrap();
        assert!(listener_holder(tcp_addr, Proto::Udp).is_none());

        let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let udp_addr = udp.local_addr().unwrap();
        assert!(listener_holder(udp_addr, Proto::Tcp).is_none());
    }
}

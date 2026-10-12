//! The 1.1.x generation: every name it left on a user's disk, in one place.
//!
//! The tool was called "AG Unlocker" until 1.1.4 and is now Open Antigravity.
//! Nothing in the product should be named after that any more - but a name that
//! was *written to a machine* is not a label, it is an address. Until a user's
//! old install is gone, the only way to find it is by its old name, so this
//! module is the single place where the previous generation still appears.
//!
//! Two jobs, both about looking backwards:
//!
//! 1. `sweep()` - removes the artifacts the previous generation installed, so
//!    a machine upgraded from it does not end up with two copies fighting over
//!    the same ports. This is not cosmetic: the older copy has been observed
//!    holding `:53` and the gate listeners while the current one stood by
//!    unable to bind, and the window blamed a Windows service for it.
//! 2. Recognition, so a file the previous generation patched is still
//!    recognised as patched. Dropping these strings would make a patched
//!    `main.js` look untouched, and the tool would patch it a second time.
//!
//! The whole module goes away once no supported upgrade path reaches back that
//! far. Nothing else in the tree may reintroduce a string from it; `src/`
//! refers to these constants instead, so the old name has exactly one home.

use std::path::PathBuf;

/// Marker written once the sweep has run, so it costs one file read per start
/// rather than a PowerShell round trip on every launch.
const SWEEP_MARKER: &str = "legacy-swept";

// --- Windows: what the previous generation installed ---

/// Scheduled tasks it registered at logon. Both ran the relay, and the relay
/// task restarts itself three times a minute - so a copy left behind does not
/// merely sit there, it keeps taking the ports back.
pub const TASK_NAME: &str = "AG Unlocker DNS";
pub const WATCHDOG_TASK_NAME: &str = "AG Unlocker Watchdog";

/// `%ProgramData%\AGUnlocker` - the install directory, with the relay exe in it.
pub const INSTALL_DIR: &str = "AGUnlocker";

/// Its image name. Also what "is this process one of ours?" answers on
/// Windows, which is how a leftover copy gets named on the port card.
pub const RELAY_IMAGE: &str = "ag_dns.exe";

/// `%LOCALAPPDATA%\AGUnlocker` - the proxy CA files it wrote there.
pub const LOCAL_DIR: &str = "AGUnlocker";

/// The Linux endpoint drop-in the previous generation wrote.
pub const ENDPOINT_DROPIN: &str = "ag-unlocker.conf";

// --- What it wrote into the system's own configuration ---

/// NRPT rule `Comment` tags. The current rules carry
/// `OPEN_ANTIGRAVITY_NRPT_V2`; these are the older, wider ones, and leaving
/// them in place would keep resolving names the new rules no longer cover.
pub const NRPT_TAGS: [&str; 2] = ["AG_UNLOCKER_NRPT_V2", "AG_UNLOCKER_NRPT"];

/// Block fences in `/etc/hosts` on the Linux port.
pub const HOSTS_FENCE_BEGIN: &str = "# AG_UNLOCKER_HOSTS_BEGIN";
pub const HOSTS_FENCE_END: &str = "# AG_UNLOCKER_HOSTS_END";

/// Trailing marker prefixes on files it rewrote.
pub const MARKER_PREFIX: &str = "// UNLOCKED";
pub const MARKER_PREFIX_ALT: &str = "// AG-UNLOCKER";

// --- What it wrote into patched IDE files ---

/// Fence pairs around the Electron hook in `main.js`, and the inner form
/// `detect_stacked_ide` looks for. A file the previous generation patched is
/// recognised by these; drop them and it would be patched a second time, on top
/// of the hook that is already there.
pub const HOOK_FENCE: (&str, &str) = ("// [AG_PROXY_HOOK]", "// [/AG_PROXY_HOOK]");
pub const HOOK_FENCE_INNER: &str = "[AG_PROXY_HOOK]";

/// Fences it left as the idempotence check on `main.js` and `extension.js`.
pub const STACKED_FENCE: &str = "[AG_PATCHED]";
pub const EXT_FENCE: &str = "[AG_EXT_PATCHED]";

/// The environment variable its Language Server build was renamed to read.
/// Same length as the current one by design, so a file the previous generation
/// patched can be moved onto the current name in place - the PE layout of a
/// 150 MB executable cannot afford a byte more.
pub const LS_GATEWAY_VAR: &str = "AG_LS_PROXY";

// --- Linux: the previous generation's unit, desktop file and data directory ---

pub const PROXY_UNIT: &str = "ag-unlocker-proxy.service";
pub const DESKTOP_FILE: &str = "ag-unlocker.desktop";
pub const DATA_DIR: &str = "agunlocker";
pub const BIN_NAME: &str = "ag_unlocker";

/// Removes the previous generation's artifacts on this machine.
///
/// Called once per install from `service::ensure_running`, before the new task
/// is registered: both copies alive at the same moment is the state that made
/// the port card lie. Idempotent, quiet about anything that is already gone,
/// and it never touches the current install - only names from this module.
pub fn sweep() {
    let marker = marker_path();
    if marker.exists() {
        return;
    }
    sweep_windows();
    sweep_unix();
    // Written last, and unconditionally: a machine where the sweep half-failed
    // must not retry it on every launch, and what is left behind is by then
    // harmless - it holds no port the new one needs.
    if let Some(dir) = marker.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    std::fs::write(&marker, b"1").ok();
}

fn marker_path() -> PathBuf {
    crate::dns_forwarder::log_dir().join(SWEEP_MARKER)
}

#[cfg(windows)]
fn sweep_windows() {
    for task in [TASK_NAME, WATCHDOG_TASK_NAME] {
        // A task that is not there answers with an error we do not care about.
        let script = format!(
            "Stop-ScheduledTask -TaskName '{task}' -ErrorAction SilentlyContinue; \
             Unregister-ScheduledTask -TaskName '{task}' -Confirm:$false -ErrorAction SilentlyContinue",
            task = task
        );
        if let Some(out) = crate::utils::powershell(&script) {
            let _ = out;
        }
    }
    // The install directory and the CA files it wrote. `installed_dir()` is the
    // current one, so guard against the two ever pointing at the same place.
    for dir in [
        std::path::PathBuf::from(program_data()).join(INSTALL_DIR),
        std::path::PathBuf::from(local_app_data()).join(LOCAL_DIR),
    ] {
        if dir == crate::platform::service::install_dir() {
            continue;
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(not(windows))]
fn sweep_windows() {}

#[cfg(unix)]
fn sweep_unix() {
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        return;
    }
    let home = PathBuf::from(home);
    let config = home.join(".config").join("systemd").join("user");
    std::fs::remove_file(config.join(PROXY_UNIT)).ok();
    let desktop = home.join(".local").join("share").join("applications");
    std::fs::remove_file(desktop.join(DESKTOP_FILE)).ok();
    std::fs::remove_file(
        home.join(".config")
            .join("environment.d")
            .join(ENDPOINT_DROPIN),
    )
    .ok();
    std::fs::remove_dir_all(home.join(".local").join("share").join(DATA_DIR)).ok();
    // A binary of the previous generation left in a directory on PATH.
    for dir in [home.join(".local").join("bin"), PathBuf::from("/usr/local/bin")] {
        std::fs::remove_file(dir.join(BIN_NAME)).ok();
    }
}

#[cfg(not(unix))]
fn sweep_unix() {}

#[cfg(windows)]
fn program_data() -> String {
    std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_string())
}

#[cfg(windows)]
fn local_app_data() -> String {
    std::env::var("LOCALAPPDATA").unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every string here exists to name something the previous generation left
    /// behind. A name that no longer matches any of them is a rename that
    /// quietly stops the sweep - so they are checked against the artifacts they
    /// are supposed to describe.
    #[test]
    fn the_previous_generation_is_named_only_where_it_has_to_be() {
        assert_eq!(RELAY_IMAGE, "ag_dns.exe");
        assert_eq!(LS_GATEWAY_VAR, "AG_LS_PROXY");
        assert_eq!(LS_GATEWAY_VAR.len(), crate::patcher::binary::PROXY_VAR_NEW.len());
        assert_eq!(HOOK_FENCE.0, "// [AG_PROXY_HOOK]");
        assert!(NRPT_TAGS.contains(&"AG_UNLOCKER_NRPT_V2"));
        assert!(NRPT_TAGS.contains(&"AG_UNLOCKER_NRPT"));
    }

    /// The sweep must never be pointed at the live install.
    #[test]
    fn the_sweep_marker_lives_beside_the_logs_not_in_the_install_dir() {
        let marker = marker_path();
        assert_eq!(marker.file_name().unwrap(), SWEEP_MARKER);
        #[cfg(target_os = "windows")]
        assert_ne!(marker.parent().unwrap(), crate::platform::service::install_dir());
        #[cfg(not(target_os = "windows"))]
        assert_ne!(marker.parent().unwrap().file_name().unwrap(), DATA_DIR);
    }
}
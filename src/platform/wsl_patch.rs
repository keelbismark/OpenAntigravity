//! Patches the Language Server that Antigravity runs *inside WSL*.
//!
//! Antigravity 2.32.x launches its language server as a Linux process under
//! `~/.antigravity-server/bin/<ver>/language_server` in the WSL distro. Those
//! binaries are separate from the Windows ones `binary_targets()` patches, so
//! they kept the stock `ineligible` check and the stock `https_proxy` lookup:
//! the Linux server never read OAG_GATEWAY, sent `aicode.googleapis.com` direct,
//! and Google answered 403 `LOCATION_NOT_ELIGIBLE / detected RU`.
//!
//! WSL2 exposes its filesystem to Windows at `\\wsl$\<distro>\...`, so the same
//! byte-exact rewrite used on Windows binaries works here without running any
//! code inside the distro. This module only discovers those files and hands them
//! to the existing patcher; it is best-effort — no WSL, no folder, or a locked
//! file must never fail the Windows patch flow.

use std::path::{Path, PathBuf};

/// Finds every language server Antigravity installed in a running WSL distro.
///
/// Queries `wsl.exe -l -q` for distro names, then looks for
/// `<drive>\\wsl$\<distro>\home\<user>\.antigravity-server\bin\*\language_server`
/// via a shallow walk of the distro's `home` tree (the user name varies).
pub fn wsl_language_servers() -> Vec<PathBuf> {
    let distros = match list_distros() {
        Some(d) => d,
        None => return Vec::new(),
    };
    let mut found = Vec::new();
    for distro in distros {
        let root = PathBuf::from(format!(r"\\wsl$\{distro}\home"));
        // Depth-limited walk: language servers live a few levels under any
        // user's home, and we must not traverse the whole distro filesystem.
        for user in read_dirs(&root) {
            let base = user.join(".antigravity-server").join("bin");
            for ver in read_dirs(&base) {
                let candidate = ver.join("language_server");
                if candidate.is_file() {
                    found.push(candidate);
                }
            }
        }
    }
    found
}

/// Patches every WSL language server with the same two rewrites used on the
/// Windows binaries: the eligibility rename and the private proxy variable.
/// Returns how many files were actually changed.
pub fn patch_all_wsl() -> usize {
    let mut changed = 0;
    for path in wsl_language_servers() {
        let gate = crate::patch_binary::patch_binary(Path::new(""), &path);
        let proxy = crate::patch_binary::patch_proxy_var(&path);
        // A signature miss on an already-patched file is Ok(0); a real I/O
        // error is skipped rather than aborting the other files.
        if gate.map(|n| n > 0).unwrap_or(false) || proxy.map(|n| n > 0).unwrap_or(false) {
            changed += 1;
            crate::dns_forwarder::log_proxy(&format!(
                "WSL: language_server пропатчен: {}",
                path.display()
            ));
        }
    }
    changed
}

fn list_distros() -> Option<Vec<String>> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("wsl.exe")
        .args(["-l", "-q"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // UTF-16LE with NULs from wsl.exe; lossy is fine for names used in a path.
    let text = String::from_utf16_lossy(&to_u16(&out.stdout));
    Some(
        text.lines()
            .map(|l| l.trim_matches(['\r', '\0']).trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
    )
}

fn to_u16(bytes: &[u8]) -> Vec<u16> {
    // wsl.exe emits UTF-16LE; pair consecutive bytes explicitly.
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i + 1 < bytes.len() {
        out.push(u16::from_le_bytes([bytes[i], bytes[i + 1]]));
        i += 2;
    }
    out
}

fn read_dirs(path: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_with_nuls_decodes_to_distro_names() {
        let mut bytes = Vec::new();
        for ch in "Ubuntu-26.04\r\ndocker-desktop\r\n".encode_utf16() {
            bytes.extend_from_slice(&ch.to_le_bytes());
        }
        let text = String::from_utf16_lossy(&to_u16(&bytes));
        let names: Vec<&str> = text
            .lines()
            .map(|l| l.trim_matches(['\r', '\0']).trim())
            .filter(|l| !l.is_empty())
            .collect();
        assert_eq!(names, vec!["Ubuntu-26.04", "docker-desktop"]);
    }

    #[test]
    fn unc_root_is_built_from_distro_name() {
        let root = PathBuf::from(format!(r"\\wsl$\{}\home", "Ubuntu-26.04"));
        assert!(root.to_string_lossy().contains("Ubuntu-26.04"));
    }
}

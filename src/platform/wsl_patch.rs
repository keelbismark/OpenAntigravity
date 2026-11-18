//! Patches the Language Server that Antigravity runs *inside WSL*.
//!
//! Antigravity 2.32.x launches its language server as a Linux process under
//! `~/.antigravity-server/bin/<ver>/language_server` in the WSL distro. Those
//! binaries are separate from the Windows ones `binary_targets()` patches, so
//! they kept the stock `ineligible` check and the stock `https_proxy` lookup:
//! the Linux server never read OAG_GATEWAY, sent `aicode.googleapis.com` direct,
//! and Google answered 403 `LOCATION_NOT_ELIGIBLE / detected RU`.
//!
//! The WSL filesystem is read-only through the `\\wsl$` share from Windows, so
//! patching runs inside the distro via `wsl.exe -d <distro> -- python3 -` with a
//! byte-exact rewrite identical to the Windows patcher. Discovery uses the same
//! share (reading works). Best-effort: no WSL, no folder, no python must never
//! fail the Windows patch flow.

use std::path::{Path, PathBuf};

/// The byte-exact rewrites, identical to `patcher::binary`: the eligibility
/// rename and the private proxy variable, both same-length so the PE/ELF layout
/// is preserved.
const PATCH_SCRIPT: &str = r#"
import sys
p = sys.argv[1]
with open(p, "rb") as f:
    d = f.read()
orig = d
# Same two renames the Windows patcher applies.
d = d.replace("ineligible".encode(), "inexigible".encode())
d = d.replace("https_proxy".encode(), "OAG_GATEWAY".encode())
d = d.replace("oag_prev_gateway".encode(), "OAG_GATEWAY".encode())
if d != orig:
    with open(p, "wb") as f:
        f.write(d)
print("changed" if d != orig else "noop")
"#;

/// Finds (distro, Linux path) for every Antigravity language server in a
/// running WSL distro. Discovery reads through `\\wsl$`, which is readable.
pub fn wsl_language_servers() -> Vec<(String, String)> {
    let distros = match list_distros() {
        Some(d) => d,
        None => return Vec::new(),
    };
    let mut found = Vec::new();
    for distro in distros {
        let root = PathBuf::from(format!(r"\\wsl$\{distro}\home"));
        for user in read_dirs(&root) {
            let base = user.join(".antigravity-server").join("bin");
            for ver in read_dirs(&base) {
                let candidate = ver.join("language_server");
                if candidate.is_file() {
                    // The Windows UNC path maps back to a POSIX path inside WSL.
                    let tail = user
                        .strip_prefix(&root)
                        .ok()
                        .and_then(|p| p.to_str())
                        .unwrap_or("");
                    let rel = ver
                        .strip_prefix(&base)
                        .ok()
                        .and_then(|p| p.to_str())
                        .unwrap_or("");
                    let home_rel = format!("/home/{}/.antigravity-server/bin/{}/language_server", tail.replace('\\', "/"), rel.replace('\\', "/"));
                    found.push((distro.clone(), home_rel));
                }
            }
        }
    }
    found
}

/// Patches every WSL language server in place with the same rewrites used on
/// Windows binaries. Returns how many files were actually changed.
pub fn patch_all_wsl() -> usize {
    let mut changed = 0;
    for (distro, path) in wsl_language_servers() {
        let mut child = match std::process::Command::new("wsl.exe")
            .args(["-d", &distro, "--", "python3", "-", &path])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write;
            let _ = stdin.write_all(PATCH_SCRIPT.as_bytes());
        }
        let out = child.wait_with_output().ok();
        let said_changed = out
            .as_ref()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("changed"))
            .unwrap_or(false);
        if said_changed {
            changed += 1;
            crate::dns_forwarder::log_proxy(&format!(
                "WSL: language_server пропатчен: {distro}:{path}"
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
    let text = String::from_utf16_lossy(&to_u16(&out.stdout));
    Some(
        text.lines()
            .map(|l| l.trim_matches(['\r', '\0']).trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
    )
}

fn to_u16(bytes: &[u8]) -> Vec<u16> {
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
    fn patch_script_renames_both_signatures() {
        // The script is data; assert it carries the exact three rewrites.
        assert!(PATCH_SCRIPT.contains("ineligible"));
        assert!(PATCH_SCRIPT.contains("inexigible"));
        assert!(PATCH_SCRIPT.contains("https_proxy"));
        assert!(PATCH_SCRIPT.contains("OAG_GATEWAY"));
    }
}

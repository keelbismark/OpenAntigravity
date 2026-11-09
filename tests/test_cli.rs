use std::process::Command;

#[test]
fn test_cli_version_flag() {
    let bin_path = env!("CARGO_BIN_EXE_open_antigravity");
    let output = Command::new(bin_path)
        .arg("--version")
        .output()
        .expect("Failed to execute open_antigravity binary with --version");

    assert!(output.status.success(), "Exit status should be successful");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);
    assert!(
        combined.contains("Open Antigravity") || combined.contains("v1.") || combined.contains("1.4"),
        "Version output expected, got: {}",
        combined
    );
}

#[test]
fn test_cli_help_flag() {
    let bin_path = env!("CARGO_BIN_EXE_open_antigravity");
    let output = Command::new(bin_path)
        .arg("--help")
        .output()
        .expect("Failed to execute open_antigravity binary with --help");

    assert!(output.status.success(), "Exit status should be successful");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);
    assert!(
        combined.contains("--help") || combined.contains("--version") || combined.contains("Open Antigravity"),
        "Help output expected, got: {}",
        combined
    );
}

#[test]
fn test_cli_about_flag() {
    let bin_path = env!("CARGO_BIN_EXE_open_antigravity");
    let output = Command::new(bin_path)
        .arg("--about")
        .output()
        .expect("Failed to execute open_antigravity binary with --about");

    assert!(output.status.success(), "Exit status should be successful");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);
    assert!(
        combined.contains("MIT") || combined.contains("Open Antigravity") || combined.contains("keelbismark"),
        "About/license output expected, got: {}",
        combined
    );
}

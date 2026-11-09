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

#[test]
fn test_cli_status_flag() {
    let bin_path = env!("CARGO_BIN_EXE_open_antigravity");
    let output = Command::new(bin_path)
        .arg("--status")
        .output()
        .expect("Failed to execute open_antigravity binary with --status");

    // Exit status can be 0 (ready) or 1 (needs patch), both are valid execution outcomes
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);
    assert!(
        combined.contains("Open Antigravity") && combined.contains("Статус готовности"),
        "Status output expected, got: {}",
        combined
    );
}

#[test]
fn test_cli_status_json_flag() {
    let bin_path = env!("CARGO_BIN_EXE_open_antigravity");
    let output = Command::new(bin_path)
        .args(&["--status", "--json"])
        .output()
        .expect("Failed to execute open_antigravity binary with --status --json");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Failed to parse --status --json output as JSON: {}. Output was: {}", e, stdout));

    assert!(parsed.get("version").is_some());
    assert!(parsed.get("platform").is_some());
    assert!(parsed.get("ready").is_some());
    assert!(parsed.get("installs").is_some());
}

#[test]
fn test_cli_check_json_flag() {
    let bin_path = env!("CARGO_BIN_EXE_open_antigravity");
    let output = Command::new(bin_path)
        .args(&["--check", "--json"])
        .output()
        .expect("Failed to execute open_antigravity binary with --check --json");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Failed to parse --check --json output as JSON: {}. Output was: {}", e, stdout));

    assert!(parsed.get("overall_status").is_some());
    assert!(parsed.get("items").is_some());
}

#[test]
fn test_cli_help_documents_headless_commands() {
    let bin_path = env!("CARGO_BIN_EXE_open_antigravity");
    let output = Command::new(bin_path)
        .arg("--help")
        .output()
        .expect("Failed to execute open_antigravity binary with --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--status"));
    assert!(stdout.contains("--patch"));
    assert!(stdout.contains("--unpatch"));
    assert!(stdout.contains("--fix"));
    assert!(stdout.contains("--check"));
}

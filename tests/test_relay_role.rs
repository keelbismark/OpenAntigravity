use std::io::BufRead;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Two relays on one machine are not a redundancy, they are a fault: both bind
/// the same addresses, and only one can win each of them, so the loser answers
/// the gate hosts with Google's real address while the winner holds their
/// door, and the two overwrite each other's record of what is going on.
///
/// The relay role is what ends that - a named object the operating system
/// hands to one holder at a time. This proves it across processes, which is
/// the unit that matters: a second relay must not be able to own the bypass
/// while the first one is serving.
///
/// The probe holds the role until it is killed, so every probe here is killed
/// when it has said what it had to say.
#[test]
fn only_one_process_can_hold_the_relay_role() {
    let bin_path = env!("CARGO_BIN_EXE_open_antigravity");

    let (mut holder, said) = probe(bin_path);
    assert_eq!(said, "taken", "the first process must take the role");

    let (mut second, said) = probe(bin_path);
    assert_eq!(
        said, "busy",
        "a second process must be refused the role while the first holds it"
    );
    second.kill().ok();
    second.wait().ok();

    // And with the holder gone, the role is free again - the failover a
    // waiting relay depends on.
    holder.kill().ok();
    holder.wait().ok();

    let (mut third, said) = probe(bin_path);
    assert_eq!(
        said, "taken",
        "the role must be free again once its holder is gone"
    );
    third.kill().ok();
    third.wait().ok();
}

/// Starts the probe and returns what it said, with the process still running.
///
/// The probe holds the role until it ends, so the caller ends it. Reading what
/// it said is bounded: a probe that said nothing and never ended must not
/// hang the test.
fn probe(bin_path: &str) -> (Child, String) {
    let mut child = Command::new(bin_path)
        .arg("--relay-role")
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start the role probe");

    let out = child.stdout.take().expect("the probe's stdout");
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut line = String::new();
        let mut reader = std::io::BufReader::new(out);
        reader.read_line(&mut line).ok();
        let _ = tx.send(line.trim().to_string());
    });

    let said = rx
        .recv_timeout(Duration::from_secs(30))
        .unwrap_or_default();
    (child, said)
}

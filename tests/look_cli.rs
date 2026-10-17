//! `vas`, the lookout, the binary, against a local mock of the account API: the columns `--wide`
//! adds, and that the machine and the host each land under their own header. No network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

mod common;

use std::process::Command;

use common::serve;

/// Three instances (the ids from the Me36 dogfooding note, box 2 and box 3, and a box the API
/// gave no ids for), then the account.
fn account(target: &str) -> (u16, String) {
    if target.starts_with("/api/v1/instances/") {
        (
            200,
            r#"{"instances": [
                {"id": 56362434, "actual_status": "running", "dph_total": 0.9, "start_date": 1.0,
                 "ssh_host": "ssh6.vast.ai", "ssh_port": 36000, "label": "me36 box 2",
                 "machine_id": 100682, "host_id": 406325},
                {"id": 56363476, "actual_status": "exited", "machine_id": "140734", "host_id": 483833},
                {"id": 56366414, "actual_status": "exited"}
              ], "next_token": null}"#
                .to_owned(),
        )
    } else if target.starts_with("/api/v0/users/current") {
        (200, r#"{"credit": 12.93}"#.to_owned())
    } else {
        (404, "{}".to_owned())
    }
}

/// `vas args` against the mock, with a key in the environment and an empty home; stdout.
fn vas(args: &[&str]) -> String {
    let (base, _) = serve(account);
    let home = std::env::temp_dir().join(format!("vas-look-{}-{}", args.len(), std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_vas"))
        .args(args)
        .env("VAST_URL", &base)
        .env("VAST_API_KEY", "k3y")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    std::fs::remove_dir_all(&home).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The whitespace-separated words of the line that contains `id`.
fn words_of<'a>(text: &'a str, id: &str) -> Vec<&'a str> {
    text.lines()
        .find(|l| l.starts_with(id))
        .unwrap_or_else(|| panic!("no row for {id}: {text}"))
        .split_whitespace()
        .collect()
}

#[test]
fn wide_shows_the_machine_then_the_host_each_under_its_own_header() {
    let text = vas(&["--wide"]);
    let header: Vec<&str> = text.lines().next().unwrap().split_whitespace().collect();
    let at = header.iter().position(|&h| h == "MACHINE").unwrap();
    assert_eq!(header[at + 1], "HOST", "{text}");
    assert_eq!(header[at - 1], "SSH", "{text}");
    // Words, not columns: the two ids sit side by side, in this order, after the ssh endpoint.
    let first = words_of(&text, "56362434");
    assert!(
        first
            .windows(3)
            .any(|w| w == ["ssh6.vast.ai:36000", "100682", "406325"]),
        "machine 100682 of host 406325: {text}"
    );
    let second = words_of(&text, "56363476");
    assert!(
        second.windows(2).any(|w| w == ["140734", "483833"]),
        "a numeric-string machine id is read too: {text}"
    );
    let third = words_of(&text, "56366414");
    assert!(
        third.windows(3).any(|w| w == ["-", "-", "-"]),
        "no ssh endpoint, no machine, no host: {text}"
    );
}

#[test]
fn without_wide_neither_id_is_shown() {
    let text = vas(&[]);
    for absent in ["MACHINE", "HOST", "100682", "406325", "140734", "483833"] {
        assert!(!text.contains(absent), "{absent} in {text}");
    }
    assert!(text.contains("credit $12.93"), "{text}");
}

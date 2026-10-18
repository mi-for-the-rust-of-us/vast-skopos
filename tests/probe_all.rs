//! `probe_all`: one outcome per instance, in order, and no `ssh` for a box that cannot be probed;
//! and the `ssh` command's arguments, which are never a shell string.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

use std::ffi::OsStr;
use std::path::Path;

use vast_skopos::instance::Instance;
use vast_skopos::probe::{probe_all, ssh_command};

fn instance(row: &serde_json::Value) -> Instance {
    Instance::from_json(row).unwrap()
}

#[test]
fn a_box_that_is_not_running_or_has_no_endpoint_is_not_probed_and_keeps_its_place() {
    let rows = [
        // Stopped, with an endpoint: not probed.
        instance(&serde_json::json!({"id": 1, "actual_status": "exited",
            "ssh_host": "ssh6.vast.ai", "ssh_port": 36000})),
        // Running, but the API gave no endpoint: nothing to ask.
        instance(&serde_json::json!({"id": 2, "actual_status": "running"})),
        // No status at all.
        instance(&serde_json::json!({"id": 3})),
    ];
    let outcomes = probe_all(&rows, None, None);
    assert_eq!(outcomes.len(), 3, "one outcome per instance");
    assert!(outcomes.iter().all(Option::is_none), "{outcomes:?}");
    assert!(probe_all(&[], None, None).is_empty());
}

#[test]
fn the_ssh_command_keeps_every_value_a_separate_argument_and_the_target_after_the_double_dash() {
    let cmd = ssh_command(
        "ssh6.vast.ai",
        36000,
        Some(Path::new("/my keys/id ed25519")),
        Some("/root/runs/*/train.log"),
    );
    assert_eq!(cmd.get_program(), "ssh");
    let args: Vec<&OsStr> = cmd.get_args().collect();
    assert_eq!(&args[..2], ["-p", "36000"]);
    for option in [
        "BatchMode=yes",
        "ConnectTimeout=10",
        "StrictHostKeyChecking=accept-new",
    ] {
        let at = args.iter().position(|a| *a == option).unwrap();
        assert_eq!(
            args[at - 1],
            "-o",
            "{option} is an option's value, not a word of its own"
        );
    }
    // Every `-o` option, each value its own argument, none missing: a probe must never ask, never
    // wait for ever, drop a dead connection, accept a first host key, and keep ssh's chatter out of
    // the reason it reports.
    let options: Vec<&OsStr> = args
        .windows(2)
        .filter(|w| w[0] == "-o")
        .map(|w| w[1])
        .collect();
    assert_eq!(
        options,
        [
            "BatchMode=yes",
            "ConnectTimeout=10",
            "ServerAliveInterval=5",
            "ServerAliveCountMax=3",
            "StrictHostKeyChecking=accept-new",
            "LogLevel=ERROR",
        ]
    );
    let at = args.iter().position(|a| *a == "-i").unwrap();
    assert_eq!(
        args[at + 1],
        "/my keys/id ed25519",
        "one path, spaces and all"
    );
    // Everything after `--` is operand: the target, then the remote command, nothing else.
    let end = args.iter().position(|a| *a == "--").unwrap();
    assert_eq!(args[end + 1], "root@ssh6.vast.ai");
    assert_eq!(args.len(), end + 3, "{args:?}");
    assert!(
        args[end + 2]
            .to_string_lossy()
            .contains("/root/runs/*/train.log"),
        "the glob goes to the box, quoted for its shell, as one argument"
    );
    // No identity, no `-i`.
    let plain = ssh_command("h", 22, None, None);
    assert!(plain.get_args().all(|a| a != "-i"));
}

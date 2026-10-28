//! `vas --instance` with `--until`, end to end: two boxes, a fake `ssh` first on the `PATH` that runs
//! the remote command with the local `sh` in a directory of the box's own (so each box has its own
//! log), and a fake `nvidia-smi`. Linux only, as `tests/until_cli.rs`.

#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use common::{KillOnDrop, RemoveOnDrop, exit_within_deadline, serve};

/// Two running boxes, 77 on `a.invalid` and 78 on `b.invalid`; and the account.
fn two_boxes(target: &str) -> (u16, String) {
    if target.starts_with("/api/v1/instances/") {
        (
            200,
            r#"{"instances": [
                {"id": 77, "actual_status": "running", "intended_status": "running", "dph_total": 0.5,
                 "start_date": 1.0, "ssh_host": "a.invalid", "ssh_port": 22},
                {"id": 78, "actual_status": "running", "intended_status": "running", "dph_total": 0.25,
                 "start_date": 1.0, "ssh_host": "b.invalid", "ssh_port": 22}],
                "next_token": null}"#
                .to_owned(),
        )
    } else {
        (200, r#"{"credit": 5.0, "balance": 0}"#.to_owned())
    }
}

/// A scratch directory: `bin` holds a fake `ssh`, which logs the box's host to `attempts` and runs
/// the remote command in `hosts/<the box's host>`, and a fake `nvidia-smi`; each host has its own
/// `runs/s0/run.log`.
fn scratch(name: &str) -> RemoveOnDrop {
    let dir = std::env::temp_dir().join(format!("vas-instance-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    for host in ["a.invalid", "b.invalid"] {
        fs::create_dir_all(dir.join("hosts").join(host).join("runs/s0")).unwrap();
    }
    let script = |file: &str, text: &str| {
        let path = bin.join(file);
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    };
    script(
        "ssh",
        &format!(
            r#"#!/bin/sh
target=
for arg; do case "$arg" in root@*) target="${{arg#root@}}" ;; esac; last="$arg"; done
echo "$target" >> '{0}/attempts'
cd '{0}/hosts/'"$target" || exit 255
exec sh -c "$last"
"#,
            dir.display()
        ),
    );
    script(
        "nvidia-smi",
        "#!/bin/sh\ncase \"$*\" in *utilization.gpu*) echo \"0, 99, 7700, 32607\" ;; esac\n",
    );
    RemoveOnDrop(dir)
}

fn log(dir: &Path, host: &str) -> std::path::PathBuf {
    dir.join("hosts").join(host).join("runs/s0/run.log")
}

#[test]
fn a_wait_on_a_box_named_ignores_a_match_on_a_box_not_named() {
    let dir = scratch("until");
    // Box 77 has the line from the start; box 78, named, has not written it yet.
    fs::write(log(&dir.0, "a.invalid"), "initial loss 9.0\n").unwrap();
    fs::write(log(&dir.0, "b.invalid"), "step 1\n").unwrap();
    let (base, _) = serve(two_boxes);
    let path = format!(
        "{}:{}",
        dir.0.join("bin").display(),
        std::env::var("PATH").unwrap()
    );
    let child = Command::new(env!("CARGO_BIN_EXE_vas"))
        .args([
            "--ssh",
            "--log",
            "runs/*/run.log",
            "--watch",
            "0",
            "--until",
            "initial loss",
            "--instance",
            "78",
        ])
        .env("VAST_URL", &base)
        .env("VAST_API_KEY", "k3y")
        .env("HOME", &dir.0)
        .env("PATH", path)
        .env("VAS_WATCH_FLOOR_MS", "50")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = KillOnDrop(Some(child));
    // Several looks: box 77's line must not end the wait.
    std::thread::sleep(std::time::Duration::from_millis(800));
    assert!(
        child.try_wait().unwrap().is_none(),
        "the wait ended on box 77"
    );
    fs::write(
        log(&dir.0, "b.invalid"),
        "step 1\ninitial loss 4.2\nstep 2\n",
    )
    .unwrap();
    let status = exit_within_deadline(&mut child);
    let (mut stdout, mut stderr) = (String::new(), String::new());
    std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut stdout).unwrap();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
    assert_eq!(status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("vas: --until: box 78 matched: initial loss 4.2"),
        "{stderr}"
    );
    assert!(
        !stdout.lines().any(|l| l.starts_with("77 ")),
        "box 77 is never shown: {stdout}"
    );
    let attempts = fs::read_to_string(dir.0.join("attempts")).unwrap();
    assert!(
        attempts.lines().all(|host| host == "b.invalid") && !attempts.is_empty(),
        "box 77 is never probed: {attempts}"
    );
    assert!(
        stdout.contains("burn $0.750/h over 2 running") && stdout.contains("| 1 of 2 boxes shown"),
        "the money is the account's: {stdout}"
    );
}

#[test]
fn without_instance_the_same_wait_ends_on_the_first_box() {
    let dir = scratch("control");
    fs::write(log(&dir.0, "a.invalid"), "initial loss 9.0\n").unwrap();
    fs::write(log(&dir.0, "b.invalid"), "step 1\n").unwrap();
    let (base, _) = serve(two_boxes);
    let path = format!(
        "{}:{}",
        dir.0.join("bin").display(),
        std::env::var("PATH").unwrap()
    );
    let out = Command::new(env!("CARGO_BIN_EXE_vas"))
        .args([
            "--ssh",
            "--log",
            "runs/*/run.log",
            "--watch",
            "0",
            "--until",
            "initial loss",
        ])
        .env("VAST_URL", &base)
        .env("VAST_API_KEY", "k3y")
        .env("HOME", &dir.0)
        .env("PATH", path)
        .env("VAS_WATCH_FLOOR_MS", "50")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("vas: --until: box 77 matched: initial loss 9.0"),
        "the fake gives each box its own log: {stderr}"
    );
}

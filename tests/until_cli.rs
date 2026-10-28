//! `vas --grep` and `vas --until`, end to end: a fake `ssh` first on the `PATH` runs the remote
//! command with the local `sh` (and a fake `nvidia-smi`), against a scratch log. Linux only, as
//! `tests/remote.rs`: the boxes are Linux, and the remote command needs `timeout`.

#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use common::{KillOnDrop, RemoveOnDrop, exit_within_deadline, serve};

/// One running box, at an address the fake `ssh` never resolves; and the account.
fn one_box(target: &str) -> (u16, String) {
    if target.starts_with("/api/v1/instances/") {
        (
            200,
            r#"{"instances": [{"id": 77, "actual_status": "running", "intended_status": "running",
                "dph_total": 0.5, "start_date": 1.0, "ssh_host": "box.invalid", "ssh_port": 22}],
                "next_token": null}"#
                .to_owned(),
        )
    } else {
        (200, r#"{"credit": 5.0, "balance": 0}"#.to_owned())
    }
}

/// A scratch directory with a `bin` holding a fake `ssh` (it runs its last argument, the remote
/// command, with `sh`) and a fake `nvidia-smi` (one busy GPU), and a `runs/s0` for the log.
fn scratch(name: &str) -> RemoveOnDrop {
    let dir = std::env::temp_dir().join(format!("vas-until-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(dir.join("runs/s0")).unwrap();
    let script = |file: &str, text: &str| {
        let path = bin.join(file);
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    };
    script(
        "ssh",
        "#!/bin/sh\nfor last; do :; done\nexec sh -c \"$last\"\n",
    );
    script(
        "nvidia-smi",
        "#!/bin/sh\ncase \"$*\" in *utilization.gpu*) echo \"0, 99, 1000, 32607\" ;; esac\n",
    );
    RemoveOnDrop(dir)
}

/// `vas --ssh --log <dir>/runs/*/run.log` and `more` flags, the fake `ssh` first on the `PATH`,
/// against the mock at `base`; stdout and stderr piped.
fn vas(base: &str, dir: &Path, more: &[&str]) -> Command {
    let path = format!(
        "{}:{}",
        dir.join("bin").display(),
        std::env::var("PATH").unwrap()
    );
    let glob = format!("{}/runs/*/run.log", dir.display());
    let mut command = Command::new(env!("CARGO_BIN_EXE_vas"));
    command
        .args(["--ssh", "--log", &glob])
        .args(more)
        .env("VAST_URL", base)
        .env("VAST_API_KEY", "k3y")
        .env("HOME", dir)
        .env("PATH", path)
        .env("VAS_WATCH_FLOOR_MS", "50")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// [`vas`] with `--watch 0 --until <pattern>`, spawned.
fn wait_for(base: &str, dir: &Path, pattern: &str) -> KillOnDrop {
    let child = vas(base, dir, &["--watch", "0", "--until", pattern])
        .spawn()
        .unwrap();
    KillOnDrop(Some(child))
}

fn stdout_of(child: &mut KillOnDrop) -> String {
    let mut text = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut text).unwrap();
    text
}

fn stderr_of(child: &mut KillOnDrop) -> String {
    let mut text = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut text).unwrap();
    text
}

fn log(dir: &Path) -> PathBuf {
    dir.join("runs/s0/run.log")
}

#[test]
fn a_line_already_there_ends_the_wait_at_the_first_look() {
    let dir = scratch("there");
    fs::write(log(&dir.0), "initial loss 4.21\neval 1\neval 2\n").unwrap();
    let (base, _) = serve(one_box);
    let mut child = wait_for(&base, &dir.0, "initial loss|Traceback");
    let status = exit_within_deadline(&mut child);
    let stderr = stderr_of(&mut child);
    assert_eq!(status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("vas: --until: box 77 matched: initial loss 4.21"),
        "{stderr}"
    );
    let stdout = stdout_of(&mut child);
    assert_eq!(
        stdout.matches("== vas, unix time").count(),
        1,
        "one look: {stdout}"
    );
    assert!(
        stdout.contains("s0/run.log: initial loss 4.21"),
        "the table shows the line that ended the wait: {stdout}"
    );
}

#[test]
fn a_path_no_box_has_a_file_for_warns_then_gives_up_with_exit_code_3() {
    // The 2026-10-26 case: a wait whose path matches no file on any box (there, rewritten by Git
    // Bash) once waited until a harness killed it.
    let dir = scratch("nofile");
    let (base, _) = serve(one_box);
    let path = format!(
        "{}:{}",
        dir.0.join("bin").display(),
        std::env::var("PATH").unwrap()
    );
    let missing = format!("{}/runs/s9/run.log", dir.0.display());
    let child = Command::new(env!("CARGO_BIN_EXE_vas"))
        .args([
            "--ssh",
            "--log",
            &missing,
            "--watch",
            "0",
            "--until",
            "loss",
            "--log-wait",
            "2",
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
    let status = exit_within_deadline(&mut child);
    let stderr = stderr_of(&mut child);
    let stdout = stdout_of(&mut child);
    assert_eq!(status.code(), Some(3), "{stderr}");
    assert_eq!(
        stderr.matches("no box has a file matching --log").count(),
        1,
        "warned once: {stderr}"
    );
    assert!(
        stderr.contains("no box has had a file matching --log for 2 s (--log-wait): giving up"),
        "{stderr}"
    );
    // Looks 50 ms apart for more than a second (whole seconds are counted): the warning's two,
    // and more.
    assert!(stdout.matches("== vas, unix time").count() > 2, "{stdout}");
}

#[test]
fn a_log_path_git_bash_rewrote_reaches_the_box_as_typed() {
    // Git Bash's root here is `/fake-git`: a path under it is the rewritten one. Both forms of
    // `EXEPATH` name it: its `bin` (an editor's terminal) and the root itself (the Start menu).
    let dir = scratch("gitbash");
    fs::write(log(&dir.0), "epoch 3\n").unwrap();
    let (base, _) = serve(one_box);
    let path = format!(
        "{}:{}",
        dir.0.join("bin").display(),
        std::env::var("PATH").unwrap()
    );
    let typed = log(&dir.0).display().to_string();
    let rewritten = format!("/fake-git{typed}");
    for exepath in ["/fake-git/bin", "/fake-git"] {
        let out = Command::new(env!("CARGO_BIN_EXE_vas"))
            .args(["--ssh", "--log", &rewritten])
            .env("VAST_URL", &base)
            .env("VAST_API_KEY", "k3y")
            .env("HOME", &dir.0)
            .env("PATH", &path)
            .env("MSYSTEM", "MINGW64")
            .env("EXEPATH", exepath)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(0), "{exepath}: {stderr}");
        assert!(
            stderr.contains(&format!(
                "Git Bash rewrote {typed} as {rewritten}; read as {typed}"
            )),
            "{exepath}: {stderr}"
        );
        assert!(
            stdout.contains("s0/run.log: epoch 3"),
            "{exepath}: {stdout}"
        );
    }
}

#[test]
fn grep_shows_the_last_matching_line_in_the_table() {
    let dir = scratch("grep");
    fs::write(
        log(&dir.0),
        "resumed from e50\nepoch 51\narchived e55\nepoch 56\n",
    )
    .unwrap();
    let (base, _) = serve(one_box);
    let out = vas(&base, &dir.0, &["--grep", "archived|resumed"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(stdout.contains("s0/run.log: archived e55"), "{stdout}");
    let plain = vas(&base, &dir.0, &[]).output().unwrap();
    let plain = String::from_utf8_lossy(&plain.stdout);
    assert!(plain.contains("s0/run.log: epoch 56"), "{plain}");
}

#[test]
fn a_pattern_the_boxs_grep_refuses_ends_the_wait_at_once_with_exit_code_2() {
    let dir = scratch("refused");
    fs::write(log(&dir.0), "step 1\n").unwrap();
    let (base, _) = serve(one_box);
    let mut child = wait_for(&base, &dir.0, "(");
    let status = exit_within_deadline(&mut child);
    let stderr = stderr_of(&mut child);
    assert_eq!(status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("vas: --until: grep on box 77 refused the pattern"),
        "{stderr}"
    );
}

#[test]
fn the_wait_goes_on_until_the_line_is_written() {
    let dir = scratch("later");
    fs::write(log(&dir.0), "step 1\n").unwrap();
    let (base, _) = serve(one_box);
    let mut child = wait_for(&base, &dir.0, "initial loss");
    // Several looks find nothing: the box is read, so the wait goes on.
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert!(
        child.try_wait().unwrap().is_none(),
        "{}",
        stderr_of(&mut child)
    );
    fs::write(log(&dir.0), "step 1\ninitial loss 3.9\nstep 2\n").unwrap();
    let status = exit_within_deadline(&mut child);
    let stderr = stderr_of(&mut child);
    assert_eq!(status.code(), Some(0), "{stderr}");
    assert!(stderr.contains("matched: initial loss 3.9"), "{stderr}");
}

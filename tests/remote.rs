//! The remote command, run by the local `sh` against a scratch tree: what `--log` reports for each
//! kind of match. Linux only: the boxes are Linux, and macOS ships no `timeout`.

#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use vast_skopos::probe::{LogReading, parse_output, remote_command};

/// A fresh scratch directory under the system temp dir, one per test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vas-remote-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// What the remote command reports for `pattern`, run by `sh` in `cwd`.
fn run_in(cwd: &Path, pattern: &str) -> LogReading {
    let out = Command::new("sh")
        .arg("-c")
        .arg(remote_command(Some(pattern)))
        .current_dir(cwd)
        .output()
        .unwrap();
    parse_output(&String::from_utf8_lossy(&out.stdout))
        .log
        .expect("the separator is always printed")
}

/// A line reading.
fn line(file: &str, line: &str) -> LogReading {
    LogReading::Line {
        file: file.to_owned(),
        line: line.to_owned(),
    }
}

#[test]
fn a_quoted_part_with_spaces_and_trailing_blank_lines() {
    let d = scratch("spaces");
    let root = d.display().to_string();
    fs::create_dir_all(d.join("my runs/s0")).unwrap();
    fs::write(d.join("my runs/s0/train.log"), "epoch 1\nepoch 2\n\n\n").unwrap();
    assert_eq!(
        run_in(&d, &format!("'{root}/my runs'/*/train.log")),
        line(&format!("{root}/my runs/s0/train.log"), "epoch 2")
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn an_empty_file() {
    let d = scratch("empty");
    let root = d.display().to_string();
    fs::write(d.join("train.log"), "").unwrap();
    assert_eq!(
        run_in(&d, &format!("{root}/*.log")),
        LogReading::Empty {
            file: format!("{root}/train.log")
        }
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn nothing_matches() {
    let d = scratch("none");
    let root = d.display().to_string();
    assert_eq!(run_in(&d, &format!("{root}/*.log")), LogReading::NoMatch);
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn the_newest_match_is_a_directory() {
    let d = scratch("dirs");
    let root = d.display().to_string();
    fs::create_dir_all(d.join("s0")).unwrap();
    assert_eq!(
        run_in(&d, &format!("{root}/*")),
        LogReading::NotAFile {
            file: format!("{root}/s0")
        }
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_last_line_that_reads_like_a_marker_is_a_line() {
    let d = scratch("spoof");
    let root = d.display().to_string();
    fs::write(d.join("train.log"), "epoch 1\nvas-no-file\n").unwrap();
    assert_eq!(
        run_in(&d, &format!("{root}/*.log")),
        line(&format!("{root}/train.log"), "vas-no-file")
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_relative_match_starting_with_a_dash() {
    let d = scratch("dash");
    fs::write(d.join("-dash.log"), "ok\n").unwrap();
    assert_eq!(run_in(&d, "*.log"), line("-dash.log", "ok"));
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_progress_bar_rewritten_for_100_kb() {
    let d = scratch("bar");
    let root = d.display().to_string();
    fs::write(
        d.join("train.log"),
        format!("{}100% done", "42%\r".repeat(25_000)),
    )
    .unwrap();
    assert_eq!(
        run_in(&d, &format!("{root}/*.log")),
        line(&format!("{root}/train.log"), "100% done")
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_progress_bar_ending_on_a_carriage_return() {
    let d = scratch("cr");
    let root = d.display().to_string();
    fs::write(d.join("train.log"), "epoch 1\nstep 1\rstep 2\r").unwrap();
    assert_eq!(
        run_in(&d, &format!("{root}/*.log")),
        line(&format!("{root}/train.log"), "step 2")
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_backslash_in_a_file_name_is_kept() {
    let d = scratch("slash");
    let root = d.display().to_string();
    fs::write(d.join("a\\tb.log"), "ok\n").unwrap();
    assert_eq!(
        run_in(&d, &format!("{root}/*.log")),
        line(&format!("{root}/a\\tb.log"), "ok")
    );
    let _ = fs::remove_dir_all(&d);
}

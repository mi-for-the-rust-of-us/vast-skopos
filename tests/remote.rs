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

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use vast_skopos::probe::{LogQuery, LogReading, parse_output, remote_command};

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
        .arg(remote_command(Some(&LogQuery::new(pattern))))
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

/// Sets the modification time of `file` to `ago` before now.
fn made_older(file: &Path, ago: std::time::Duration) {
    let when = std::time::SystemTime::now() - ago;
    fs::File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

#[test]
fn the_newest_match_wins_by_modification_time_not_by_name() {
    let d = scratch("newest");
    let root = d.display().to_string();
    fs::write(d.join("a.log"), "from a\n").unwrap();
    fs::write(d.join("b.log"), "from b\n").unwrap();
    // `a.log` sorts first and is the newer: a listing by name would pick `b.log` (or `a.log`, by luck).
    made_older(&d.join("b.log"), std::time::Duration::from_secs(7_200));
    assert_eq!(
        run_in(&d, &format!("{root}/*.log")),
        line(&format!("{root}/a.log"), "from a")
    );
    // And the other way round, so that the answer follows the clock and not the names.
    made_older(&d.join("a.log"), std::time::Duration::from_secs(14_400));
    assert_eq!(
        run_in(&d, &format!("{root}/*.log")),
        line(&format!("{root}/b.log"), "from b")
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn only_the_end_of_a_long_file_is_read_and_a_last_line_needs_no_newline() {
    let d = scratch("long");
    let root = d.display().to_string();
    // Fifty lines, then blank ones, then a last line with no newline at all.
    let mut text = String::new();
    for n in 1..=50 {
        writeln!(text, "line {n}").unwrap();
    }
    text.push_str("\n\n   \nline 51");
    fs::write(d.join("train.log"), text).unwrap();
    assert_eq!(
        run_in(&d, &format!("{root}/*.log")),
        line(&format!("{root}/train.log"), "line 51")
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_last_line_longer_than_the_byte_cap_shows_its_end_cut_to_the_width() {
    let d = scratch("wide");
    let root = d.display().to_string();
    // 5000 bytes on ONE line, ending in a newline: the box keeps the last 4096 of them.
    fs::write(d.join("train.log"), format!("{}\n", "x".repeat(5_000))).unwrap();
    let LogReading::Line { line: shown, .. } = run_in(&d, &format!("{root}/*.log")) else {
        panic!("a line was expected");
    };
    assert!(shown.starts_with("xxxx") && shown.ends_with('…'), "{shown}");
    assert_eq!(shown.chars().count(), 121, "120 characters and an ellipsis");
    let _ = fs::remove_dir_all(&d);
}

/// A directory with an executable `nvidia-smi` that answers like a driver whose names for the clock
/// event reasons are those `mode` says: `new` (both answer), `old` (only `clocks_throttle_reasons`),
/// `none` (neither). Invalid fields exit 2 and print their error on STDOUT, as the real one does.
fn fake_nvidia_smi(name: &str, mode: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let d = scratch(name);
    let script = format!(
        r#"#!/bin/sh
case "$*" in
  *utilization.gpu*) echo "0, 100, 24809, 32607" ;;
  *power.draw*) echo "0, 399.86, 400.00, 575.00, 1695, 3090" ;;
  *clocks_event_reasons*)
    if [ "{mode}" = new ]; then echo "0, 0x0000000000000004"; else echo 'Field "clocks_event_reasons.active" is not a valid field to query.'; exit 2; fi ;;
  *clocks_throttle_reasons*)
    if [ "{mode}" != none ]; then echo "0, 0x0000000000000004"; else echo 'Field "clocks_throttle_reasons.active" is not a valid field to query.'; exit 2; fi ;;
  *pci.bus_id*) echo "0, 00000000:C1:00.0" ;;
  *compute-apps*) echo "00000000:C1:00.0, 8720" ;;
esac
"#
    );
    let file = d.join("nvidia-smi");
    fs::write(&file, script).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    d
}

/// What the remote command reads, run by `sh` with `bin` first on the `PATH`.
fn probe_with(bin: &Path) -> vast_skopos::probe::Probe {
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let out = Command::new("sh")
        .arg("-c")
        .arg(remote_command(None))
        .env("PATH", path)
        .output()
        .unwrap();
    parse_output(&String::from_utf8_lossy(&out.stdout))
}

#[test]
fn a_driver_that_knows_the_reasons_by_either_name_or_by_neither_still_gives_the_power() {
    for (mode, reasons) in [("new", Some(4)), ("old", Some(4)), ("none", None)] {
        let bin = fake_nvidia_smi(&format!("smi-{mode}"), mode);
        let probe = probe_with(&bin);
        let g = &probe.gpus[0];
        assert_eq!(g.throttle_mask, reasons, "{mode}");
        assert_eq!(
            (
                g.util_pct,
                g.power_limit_w,
                g.power_default_w,
                g.sm_mhz,
                g.sm_max_mhz
            ),
            (100, Some(400), Some(575), Some(1695), Some(3090)),
            "{mode}: the rest is read whatever the reasons' name"
        );
        assert_eq!(g.other_user_mib(), Some(16_089), "{mode}");
        let _ = fs::remove_dir_all(&bin);
    }
}

/// What the remote command reports for `path` searched for `pattern` (`--grep`), run by `sh` in `cwd`.
fn grep_in(cwd: &Path, path: &str, pattern: &str) -> LogReading {
    let query = LogQuery::new(path).grep(pattern);
    let out = Command::new("sh")
        .arg("-c")
        .arg(remote_command(Some(&query)))
        .current_dir(cwd)
        .output()
        .unwrap();
    parse_output(&String::from_utf8_lossy(&out.stdout))
        .log
        .expect("the separator is always printed")
}

#[test]
fn grep_finds_the_last_matching_line_not_the_last_line() {
    let d = scratch("grep-last");
    let root = d.display().to_string();
    let file = format!("{root}/train.log");
    fs::write(
        &file,
        "initial loss 9.99\nstep 1\ninitial loss 4.21\neval 1\neval 2\n",
    )
    .unwrap();
    assert_eq!(
        grep_in(&d, &format!("{root}/*.log"), "initial loss"),
        line(&file, "initial loss 4.21")
    );
    // An alternation: wait for success OR failure in one pattern.
    fs::write(
        &file,
        "initial loss 4.21\nTraceback (most recent call last):\n  x\n",
    )
    .unwrap();
    assert_eq!(
        grep_in(&d, &format!("{root}/*.log"), "initial loss|Traceback"),
        line(&file, "Traceback (most recent call last):")
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn grep_reads_each_carriage_return_segment_as_a_line() {
    let d = scratch("grep-bar");
    let root = d.display().to_string();
    let file = format!("{root}/train.log");
    fs::write(&file, "10%|#\r50%|## eta 3m\r100%|### done\nnext\n").unwrap();
    assert_eq!(
        grep_in(&d, &format!("{root}/*.log"), "50%"),
        line(&file, "50%|## eta 3m")
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn grep_says_when_no_line_matches_and_when_it_refuses_the_pattern() {
    let d = scratch("grep-none");
    let root = d.display().to_string();
    let file = format!("{root}/train.log");
    fs::write(&file, "step 1\nstep 2\n").unwrap();
    assert_eq!(
        grep_in(&d, &format!("{root}/*.log"), "initial loss"),
        LogReading::NoMatchingLine { file: file.clone() }
    );
    assert_eq!(
        grep_in(&d, &format!("{root}/*.log"), "("),
        LogReading::PatternRefused { file: file.clone() }
    );
    // An empty file has no line to match.
    fs::write(&file, "").unwrap();
    assert_eq!(
        grep_in(&d, &format!("{root}/*.log"), "step"),
        LogReading::NoMatchingLine { file }
    );
    // And nothing matched by the path is said as before.
    assert_eq!(
        grep_in(&d, &format!("{root}/*.txt"), "step"),
        LogReading::NoMatch
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_nul_byte_does_not_turn_the_answer_into_binary_file_matches() {
    let d = scratch("grep-nul");
    let root = d.display().to_string();
    let file = format!("{root}/train.log");
    fs::write(&file, b"a\0b loss 1.5\nstep\n").unwrap();
    assert_eq!(
        grep_in(&d, &format!("{root}/*.log"), "loss"),
        line(&file, "ab loss 1.5")
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn the_pattern_is_never_run_by_the_boxs_shell() {
    let d = scratch("grep-quote");
    let root = d.display().to_string();
    let file = format!("{root}/train.log");
    let canary = d.join("canary");
    let text = "it's `touch canary` ; touch canary -v $HOME\n";
    fs::write(&file, text).unwrap();
    let whole = text.trim_end();
    for pattern in [
        "it's",
        "`touch canary`",
        "; touch canary",
        "-v",
        "\\$HOME",
        "'; touch canary; echo '",
    ] {
        let read = grep_in(&d, &format!("{root}/*.log"), pattern);
        assert!(!canary.exists(), "{pattern} ran on the box");
        if pattern.starts_with("';") {
            assert_eq!(read, LogReading::NoMatchingLine { file: file.clone() });
        } else {
            assert_eq!(read, line(&file, whole), "{pattern}");
        }
    }
    // `$(...)` is an anchor and a group to grep, and nothing to the shell.
    let read = grep_in(&d, &format!("{root}/*.log"), "$(touch canary)");
    assert!(!canary.exists());
    assert_eq!(read, LogReading::NoMatchingLine { file });
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_status_line_written_in_the_log_is_only_a_line() {
    let d = scratch("grep-forge");
    let root = d.display().to_string();
    let file = format!("{root}/train.log");
    fs::write(&file, "vas-grep-exit:0\nstep 1\n").unwrap();
    assert_eq!(
        grep_in(&d, &format!("{root}/*.log"), "initial loss"),
        LogReading::NoMatchingLine { file: file.clone() }
    );
    assert_eq!(
        grep_in(&d, &format!("{root}/*.log"), "vas-grep"),
        line(&file, "vas-grep-exit:0")
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_matching_line_longer_than_the_byte_cap_keeps_its_status_and_is_cut_to_the_width() {
    let d = scratch("grep-wide");
    let root = d.display().to_string();
    fs::write(
        d.join("train.log"),
        format!("loss {}\nstep\n", "x".repeat(10_000)),
    )
    .unwrap();
    let LogReading::Line { line: shown, .. } = grep_in(&d, &format!("{root}/*.log"), "loss") else {
        panic!("a line was expected");
    };
    assert!(shown.starts_with("xxxx") && shown.ends_with('…'), "{shown}");
    assert_eq!(shown.chars().count(), 121);
    let _ = fs::remove_dir_all(&d);
}

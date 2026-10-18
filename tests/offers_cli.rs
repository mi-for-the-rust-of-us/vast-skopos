//! `vas offers`, the binary, against a local mock of the search: the identity, the config file
//! merged with the command line, and what stops a run. No key anywhere, no network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

mod common;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::{Request, decode_q, fixture, serve};

/// Every target answered with the fixture's real rows.
fn bundles(_: &str) -> (u16, String) {
    (200, fixture("offers.json"))
}

/// A fresh, empty home for one test: no key file, and no config file but the one it writes.
fn fresh_home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vas-offers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes `text` as the config file under `dir` (`dir/vast-skopos/offers`), and returns its path.
fn write_config(dir: &Path, text: &str) -> PathBuf {
    let file = dir.join("vast-skopos").join("offers");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, text).unwrap();
    file
}

/// `vas args` against `base`, with `home` as the home, `home/xdg` as `XDG_CONFIG_HOME` when `xdg`,
/// no key in the environment but what `env` adds; the home is kept. Returns the output, stdout,
/// stderr.
fn vas_in(
    base: &str,
    home: &Path,
    xdg: bool,
    env: &[(&str, &str)],
    args: &[&str],
) -> (Output, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vas"));
    command
        .args(args)
        .env("VAST_URL", base)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("VAST_API_KEY");
    if xdg {
        command.env("XDG_CONFIG_HOME", home.join("xdg"));
    } else {
        command.env_remove("XDG_CONFIG_HOME");
    }
    for &(name, value) in env {
        command.env(name, value);
    }
    let out = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (out, stdout, stderr)
}

/// [`vas_in`] with no added environment; then the home is removed.
fn vas(base: &str, home: &Path, xdg: bool, args: &[&str]) -> (Output, String, String) {
    let ran = vas_in(base, home, xdg, &[], args);
    std::fs::remove_dir_all(home).unwrap();
    ran
}

/// The one request the mock saw.
fn the_request(seen: &common::Seen) -> Request {
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "one search, no retry");
    seen[0].clone()
}

/// The askesis renting filters, as a config file.
const ASKESIS: &str = "# askesis's renting filters\n\
                       --gpu RTX_5090\n\
                       --exclude-country CN\n\
                       --reliability=0.99\n\
                       --verified\n\
                       --top 1\n";

#[test]
fn with_no_constraint_the_search_is_the_identity_and_says_so() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("identity");
    let (out, stdout, stderr) = vas(&base, &home, true, &["offers"]);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("no constraint given"), "{stderr}");
    assert!(!stderr.contains("API key"), "{stderr}");
    assert_eq!(
        stdout.lines().count(),
        1 + 5 + 1,
        "header, rows, footer: {stdout}"
    );
    assert!(
        stdout.contains("5 offers, cheapest first | no constraint | $/h includes 5 GB of storage"),
        "{stdout}"
    );
    let request = the_request(&seen);
    assert_eq!(request.method, "GET");
    assert_eq!(request.authorization, None, "the search sends no key");
    assert_eq!(
        decode_q(&request.target),
        serde_json::json!({"rentable": {"eq": true}, "rented": {"eq": false},
          "type": "on-demand", "limit": 200, "allocated_storage": 5,
          "order": [["dph_total", "asc"]]})
    );
}

#[test]
fn the_config_file_comes_first_and_the_command_line_after() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("merge");
    let file = write_config(&home.join("xdg"), ASKESIS);
    let (out, stdout, stderr) = vas(
        &base,
        &home,
        true,
        &[
            "offers",
            "--exclude-country",
            "ru",
            "--reliability",
            "0.9",
            "--no-verified",
            "--top",
            "2",
            "--sort",
            "reliability",
        ],
    );
    assert!(out.status.success(), "{stderr}");
    assert!(
        stderr.contains(&format!("the flags of {} first", file.display())),
        "{stderr}"
    );
    let q = decode_q(&the_request(&seen).target);
    assert_eq!(
        q["gpu_name"],
        serde_json::json!({"eq": "RTX 5090"}),
        "the file's"
    );
    assert_eq!(
        q["geolocation"],
        serde_json::json!({"notin": ["CN", "RU"]}),
        "the lists accumulate"
    );
    assert_eq!(
        q["reliability2"]["gte"].as_f64(),
        Some(0.9),
        "the command line wins"
    );
    assert!(
        q.get("verified").is_none(),
        "--no-verified cancels the file's --verified"
    );
    assert_eq!(q["order"], serde_json::json!([["reliability2", "desc"]]));
    // The fixture's three 5090s pass 0.9; the two most reliable are shown.
    let ids: Vec<&str> = stdout
        .lines()
        .skip(1)
        .take(2)
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(ids, ["44614424", "51179006"], "{stdout}");
    assert!(
        stdout.contains("2 of 3 offers shown, most reliable first"),
        "{stdout}"
    );
}

#[test]
fn the_home_config_file_is_read_without_xdg() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("home");
    write_config(&home.join(".config"), "--gpu RTX_5090\n");
    let (out, _, stderr) = vas(&base, &home, false, &["offers"]);
    assert!(out.status.success(), "{stderr}");
    let q = decode_q(&the_request(&seen).target);
    assert_eq!(q["gpu_name"], serde_json::json!({"eq": "RTX 5090"}));
}

#[test]
fn no_config_reads_no_file() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("noconfig");
    write_config(&home.join("xdg"), ASKESIS);
    let (out, _, stderr) = vas(&base, &home, true, &["offers", "--no-config"]);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("no constraint given"), "{stderr}");
    assert!(
        decode_q(&the_request(&seen).target)
            .get("gpu_name")
            .is_none()
    );
}

#[test]
fn a_named_config_file_is_read_and_a_missing_one_stops_the_run() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("named");
    let file = write_config(&home.join("elsewhere"), "--num-gpus 2\n");
    let missing = home.join("missing");
    let (found, _, _) = vas_in(
        &base,
        &home,
        true,
        &[],
        &["offers", "--config", file.to_str().unwrap()],
    );
    let (out, _, stderr) = vas(
        &base,
        &home,
        true,
        &["offers", "--config", missing.to_str().unwrap()],
    );
    assert!(found.status.success());
    assert_eq!(
        decode_q(&the_request(&seen).target)["num_gpus"],
        serde_json::json!({"eq": 2})
    );
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("failed to read the config file"),
        "{stderr}"
    );
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "no search after the missing file"
    );
}

#[test]
fn a_malformed_config_line_stops_the_run_before_any_search() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("malformed");
    write_config(&home.join("xdg"), "--gpu RTX_5090\ngpu RTX_4090\n");
    let (out, _, stderr) = vas(&base, &home, true, &["offers"]);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("line 2 of the config file") && stderr.contains("(not a flag"),
        "{stderr}"
    );
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn an_unknown_flag_in_the_config_file_names_the_file() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("unknown");
    let file = write_config(&home.join("xdg"), "--frobnicate 3\n");
    let (out, _, stderr) = vas(&base, &home, true, &["offers"]);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("--frobnicate")
            && stderr.contains(&format!("the flags of {} were read first", file.display())),
        "{stderr}"
    );
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn lookout_flags_are_refused_with_offers() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("conflict");
    let (out, _, stderr) = vas(&base, &home, true, &["--ssh", "offers"]);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn a_reliability_in_percent_is_refused_with_the_fraction_to_write() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("percent");
    let (out, _, stderr) = vas(&base, &home, true, &["offers", "--reliability", "99"]);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("0.99 for 99 %"), "{stderr}");
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn a_key_present_is_never_sent_nor_shown() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("withkey");
    for dir in [
        home.join(".config").join("vastai"),
        home.join("xdg").join("vastai"),
    ] {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("vast_api_key"), "s3cret-file\n").unwrap();
    }
    let (out, stdout, stderr) = vas_in(
        &base,
        &home,
        true,
        &[("VAST_API_KEY", "s3cret-env")],
        &["offers", "--gpu", "RTX_5090"],
    );
    std::fs::remove_dir_all(&home).unwrap();
    assert!(out.status.success(), "{stderr}");
    assert_eq!(
        the_request(&seen).authorization,
        None,
        "a key at hand, none sent"
    );
    assert!(
        !stdout.contains("s3cret") && !stderr.contains("s3cret"),
        "{stdout}{stderr}"
    );
}

#[test]
fn config_equals_form_and_a_value_named_offers_split_right() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("equals");
    write_config(&home.join("xdg"), "--gpu RTX_5090\n");
    let named = write_config(&home.join("elsewhere"), "--num-gpus 2\n");
    let config = format!("--config={}", named.display());
    let (out, _, stderr) = vas(&base, &home, true, &["offers", &config, "--gpu", "offers"]);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stderr.contains(&format!("the flags of {} first", named.display())),
        "{stderr}"
    );
    let q = decode_q(&the_request(&seen).target);
    assert_eq!(
        q["num_gpus"],
        serde_json::json!({"eq": 2}),
        "the named file's"
    );
    assert_eq!(
        q["gpu_name"],
        serde_json::json!({"eq": "offers"}),
        "the command line's value, not the default file's"
    );
}

#[test]
fn help_in_the_config_file_stops_the_run() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("help");
    write_config(&home.join("xdg"), "--help\n");
    let (out, stdout, stderr) = vas(&base, &home, true, &["offers"]);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("prints and stops"), "{stderr}");
    assert!(stdout.is_empty() && seen.lock().unwrap().is_empty());
}

#[test]
fn xdg_wins_over_the_home_config_and_falls_back_when_absent() {
    let (base, seen) = serve(bundles);
    let both = fresh_home("both");
    write_config(&both.join("xdg"), "--num-gpus 4\n");
    write_config(&both.join(".config"), "--num-gpus 8\n");
    let (first, _, stderr) = vas(&base, &both, true, &["offers"]);
    assert!(first.status.success(), "{stderr}");
    let only_home = fresh_home("onlyhome");
    write_config(&only_home.join(".config"), "--num-gpus 8\n");
    let (second, _, stderr) = vas(&base, &only_home, true, &["offers"]);
    assert!(second.status.success(), "{stderr}");
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    assert_eq!(
        decode_q(&seen[0].target)["num_gpus"],
        serde_json::json!({"eq": 4})
    );
    assert_eq!(
        decode_q(&seen[1].target)["num_gpus"],
        serde_json::json!({"eq": 8})
    );
}

#[test]
fn excluded_hosts_accumulate_too() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("hosts");
    write_config(&home.join("xdg"), "--exclude-host 362355\n");
    let (out, _, stderr) = vas(&base, &home, true, &["offers", "--exclude-host", "1"]);
    assert!(out.status.success(), "{stderr}");
    assert_eq!(
        decode_q(&the_request(&seen).target)["host_id"],
        serde_json::json!({"notin": [1, 362_355]})
    );
}

#[test]
fn excluded_machines_accumulate_too_and_the_footer_names_them_and_the_time() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("machines");
    write_config(
        &home.join("xdg"),
        "--exclude-machine 142875
",
    );
    let (out, _, stderr) = vas(&base, &home, true, &["offers", "--exclude-machine", "1"]);
    assert!(out.status.success(), "{stderr}");
    assert_eq!(
        decode_q(&the_request(&seen).target)["machine_id"],
        serde_json::json!({"notin": [1, 142_875]})
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let footer = stdout
        .lines()
        .find(|l| l.contains("not machines"))
        .unwrap_or("");
    assert!(footer.contains("not machines 1 142875"), "{stdout}");
    let (_, at) = footer.rsplit_once("searched ").unwrap_or(("", ""));
    let digits: Vec<bool> = at.chars().map(|c| c.is_ascii_digit()).collect();
    assert!(
        at.ends_with(" UTC")
            && at.len() == "15:42:07 UTC".len()
            && digits[..2] == [true, true]
            && at.as_bytes()[2] == b':',
        "the search time, UTC, closes the first footer line: {footer}"
    );
}

#[test]
fn a_bad_value_in_the_config_file_names_the_file() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("badvalue");
    let file = write_config(&home.join("xdg"), "--reliability 99\n");
    let (out, _, stderr) = vas(&base, &home, true, &["offers"]);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("0.99 for 99 %")
            && stderr.contains(&format!("the flags of {} were read first", file.display())),
        "{stderr}"
    );
    assert!(seen.lock().unwrap().is_empty());
}

/// The fixture's answer; from the second search on, one offer is gone (the first of the file), one
/// is repriced (+ $0.10/h) and one is new (id 999001, a copy of another).
fn evolved(call: usize) -> String {
    let mut answer: serde_json::Value = serde_json::from_str(&fixture("offers.json")).unwrap();
    if call > 0 {
        let offers = answer["offers"].as_array_mut().unwrap();
        let mut newcomer = offers[1].clone();
        newcomer["id"] = 999_001.into();
        offers.remove(0);
        let price = offers[1]["dph_total"].as_f64().unwrap();
        offers[1]["dph_total"] = (price + 0.1).into();
        offers.push(newcomer);
    }
    answer.to_string()
}

/// One counter per test that watches: the tests run in parallel, in one process.
static TABLE_LOOKS: AtomicUsize = AtomicUsize::new(0);
static CHANGES_LOOKS: AtomicUsize = AtomicUsize::new(0);

fn evolving_table(_: &str) -> (u16, String) {
    (200, evolved(TABLE_LOOKS.fetch_add(1, Ordering::SeqCst)))
}

fn evolving_changes(_: &str) -> (u16, String) {
    (200, evolved(CHANGES_LOOKS.fetch_add(1, Ordering::SeqCst)))
}

/// `vas offers --no-config --watch 0 ARGS` against `base`, with the 30 s floor of `--watch` lowered
/// to 50 ms; its stdout lines until one containing `marker` has been seen `times` times; then the
/// process is killed.
fn watch_lines(base: &str, name: &str, args: &[&str], marker: &str, times: usize) -> Vec<String> {
    let home = fresh_home(name);
    let mut child = Command::new(env!("CARGO_BIN_EXE_vas"))
        .args(["offers", "--no-config", "--watch", "0"])
        .args(args)
        .env("VAST_URL", base)
        .env("VAS_WATCH_FLOOR_MS", "50")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env_remove("VAST_API_KEY")
        .env_remove("XDG_CONFIG_HOME")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    // One deadline for the whole read, not a timeout per line: a `vas` that never reaches the marker
    // but keeps printing must fail the test, not feed it forever.
    let deadline = Instant::now() + Duration::from_secs(20);
    let (mut lines, mut seen) = (Vec::new(), 0);
    while seen < times {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        match rx.recv_timeout(left) {
            Ok(line) => {
                seen += usize::from(line.contains(marker));
                lines.push(line);
            }
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    std::fs::remove_dir_all(&home).unwrap();
    assert_eq!(seen, times, "{marker:?} seen {seen} times in {lines:#?}");
    lines
}

#[test]
fn a_watch_marks_what_moved_between_looks_and_lists_what_is_gone() {
    let (base, seen) = serve(evolving_table);
    // The notes closing the second and the third look.
    let lines = watch_lines(&base, "watch-table", &[], "since the last look", 2);
    let text = lines.join("\n");
    assert!(
        seen.lock().unwrap().len() >= 3,
        "one search per look, never fewer"
    );
    // The first look has a mark column and nothing marked.
    assert!(lines[0].trim_start().starts_with("ID"), "{text}");
    assert!(
        lines[1..6].iter().all(|l| !l.starts_with('+')),
        "nothing to mark at the first look: {text}"
    );
    // The second: the newcomer marked, the repriced offer showing its old price, the gone one named.
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("+ ") && l.contains("999001")),
        "{text}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("51054445") && l.contains("(was 0.672)")),
        "{text}"
    );
    assert!(
        text.contains("since the last look: 1 new, 1 repriced, 1 gone\n- 44614424  1x RTX 5090"),
        "{text}"
    );
    // The third look agrees with the second.
    assert!(
        lines
            .last()
            .unwrap()
            .starts_with("since the last look: no change"),
        "{text}"
    );
}

#[test]
fn changes_prints_the_baseline_then_only_what_moved() {
    let (base, _) = serve(evolving_changes);
    let lines = watch_lines(&base, "watch-changes", &["--changes"], "searched ", 3);
    let text = lines.join("\n");
    assert!(!text.contains("VERIFICATION"), "no table at all: {text}");
    assert!(
        lines[0].contains("5 offers (the first look: what moves follows)"),
        "{text}"
    );
    assert!(
        lines[1].contains("5 offers, 1 new, 1 repriced, 1 gone"),
        "{text}"
    );
    assert!(lines[2].starts_with("+ 999001  1x RTX 5090"), "{text}");
    assert!(
        lines[3].starts_with("~ 51054445  1x RTX 5090  0.672 -> 0.772 $/h"),
        "{text}"
    );
    assert!(lines[4].starts_with("- 44614424  1x RTX 5090"), "{text}");
    assert!(lines[5].contains("5 offers, no change"), "{text}");
}

#[test]
fn changes_needs_a_watch() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("changes-alone");
    let (out, _, stderr) = vas(&base, &home, true, &["offers", "--changes"]);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("--watch"), "{stderr}");
    assert!(seen.lock().unwrap().is_empty(), "refused before any search");
}

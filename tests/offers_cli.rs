//! `vas offers`, the binary, against a local mock of the search: the identity, the config file
//! merged with the command line, what stops a run, and the watch (`--watch`, `--changes`, the floor,
//! a closed pipe). No key anywhere, no network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{RemoveOnDrop, Request, decode_q, fixture, read_lines_until, serve};

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
    let _cleanup = RemoveOnDrop(home);
    read_lines_until(&mut child, marker, times)
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
    // A blank line parts the looks: the second header is preceded by one, the first by nothing.
    let headers: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.trim_start().starts_with("ID"))
        .map(|(i, _)| i)
        .collect();
    assert!(headers.len() >= 2, "two looks: {text}");
    assert_eq!(headers[0], 0, "{text}");
    assert_eq!(
        lines[headers[1] - 1],
        "",
        "a blank line before the second look: {text}"
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

/// Every search refused with a 500.
fn refusing(_: &str) -> (u16, String) {
    (500, "{}".to_owned())
}

/// The first search answers, the second is refused, the next ones answer with the evolved market.
static HICCUP_CALLS: AtomicUsize = AtomicUsize::new(0);

fn hiccup(_: &str) -> (u16, String) {
    match HICCUP_CALLS.fetch_add(1, Ordering::SeqCst) {
        0 => (200, evolved(0)),
        1 => (500, "{}".to_owned()),
        _ => (200, evolved(1)),
    }
}

/// The evolved market, for the test that watches with `--top 1`.
static TOP_LOOKS: AtomicUsize = AtomicUsize::new(0);

fn evolving_top(_: &str) -> (u16, String) {
    (200, evolved(TOP_LOOKS.fetch_add(1, Ordering::SeqCst)))
}

/// Every search counted, the answer never changing.
static FLOOR_CALLS: AtomicUsize = AtomicUsize::new(0);

fn counted_bundles(_: &str) -> (u16, String) {
    FLOOR_CALLS.fetch_add(1, Ordering::SeqCst);
    (200, fixture("offers.json"))
}

#[test]
fn a_watch_whose_first_search_fails_gives_up_instead_of_looping_on_stderr() {
    let (base, seen) = serve(refusing);
    let home = fresh_home("watch-refused");
    let mut child = Command::new(env!("CARGO_BIN_EXE_vas"))
        .args(["offers", "--no-config", "--watch", "0"])
        .env("VAST_URL", &base)
        .env("VAS_WATCH_FLOOR_MS", "50")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env_remove("VAST_API_KEY")
        .env_remove("XDG_CONFIG_HOME")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let status = common::exit_within_deadline(&mut child);
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
    std::fs::remove_dir_all(&home).unwrap();
    assert_eq!(status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("status 500"), "{stderr}");
    assert_eq!(seen.lock().unwrap().len(), 1, "one search, no retry loop");
}

#[test]
fn a_watch_goes_on_after_a_failure_once_a_look_has_succeeded_and_says_so() {
    let (base, _) = serve(hiccup);
    // The notes closing the look after the failed search, and the look after that.
    let lines = watch_lines(&base, "watch-hiccup", &[], "since the last", 2);
    let notes: Vec<&String> = lines
        .iter()
        .filter(|l| l.starts_with("since the last"))
        .collect();
    assert_eq!(
        notes[0], "since the last successful look: 1 new, 1 repriced, 1 gone",
        "the look before the refused search is the one compared with: {lines:#?}"
    );
    assert_eq!(notes[1], "since the last look: no change");
}

#[test]
fn a_watch_compares_the_whole_list_not_the_rows_it_shows() {
    let (base, _) = serve(evolving_top);
    let lines = watch_lines(
        &base,
        "watch-top",
        &["--top", "1"],
        "since the last look",
        1,
    );
    let note = lines
        .iter()
        .find(|l| l.starts_with("since the last look"))
        .unwrap();
    // One row is on screen; the newcomer and the repriced offer are among the 4 below it, and the
    // gone offer is gone from the whole list, not just from the screen.
    assert_eq!(
        note,
        "since the last look: 1 new (1 below the table), 1 repriced (1 below the table), 1 gone",
        "{lines:#?}"
    );
}

#[test]
fn a_config_file_cannot_hold_a_watch() {
    let (base, seen) = serve(bundles);
    for flag in ["--watch 60", "--changes"] {
        let home = fresh_home("config-watch");
        let file = write_config(&home.join("xdg"), &format!("--gpu RTX_5090\n{flag}\n"));
        // Spawned, not run to completion: were the flag accepted, `vas` would loop, and the test
        // must fail at the deadline, not hang the suite.
        let mut child = Command::new(env!("CARGO_BIN_EXE_vas"))
            .arg("offers")
            .env("VAST_URL", &base)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_CONFIG_HOME", home.join("xdg"))
            .env_remove("VAST_API_KEY")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let status = common::exit_within_deadline(&mut child);
        let mut stderr = String::new();
        std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
        std::fs::remove_dir_all(&home).unwrap();
        assert_eq!(status.code(), Some(1), "{flag}: {stderr}");
        assert!(
            stderr.contains(&format!("line 2 of the config file {}", file.display()))
                && stderr.contains("must not turn a run into a loop"),
            "{flag}: {stderr}"
        );
    }
    assert!(seen.lock().unwrap().is_empty(), "refused before any search");
}

#[test]
fn the_pause_between_two_looks_is_at_least_thirty_seconds_unless_a_test_says_otherwise() {
    let (base, seen) = serve(counted_bundles);
    let home = fresh_home("floor");
    let mut child = Command::new(env!("CARGO_BIN_EXE_vas"))
        .args(["offers", "--no-config", "--watch", "1"])
        .env("VAST_URL", &base)
        .env_remove("VAS_WATCH_FLOOR_MS")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env_remove("VAST_API_KEY")
        .env_remove("XDG_CONFIG_HOME")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Wait for the first search however long the process takes to start (a scanned fresh exe, a
    // loaded runner), THEN 1.5 s: past the 1 s asked, far short of the 30 s floor.
    let started = std::time::Instant::now();
    while FLOOR_CALLS.load(Ordering::SeqCst) == 0 {
        assert!(
            started.elapsed() < common::PROCESS_DEADLINE,
            "the first search never came"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let _ = child.kill();
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&home);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        FLOOR_CALLS.load(Ordering::SeqCst),
        1,
        "the second search waits for the floor"
    );
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(
        stderr.contains("--watch 1 is raised to 30 s"),
        "the raise is said once, on stderr: {stderr}"
    );
}

#[test]
fn a_search_of_this_machine_does_not_go_through_the_proxy_of_the_environment() {
    let (base, seen) = serve(bundles);
    let home = fresh_home("proxy");
    let mut command = Command::new(env!("CARGO_BIN_EXE_vas"));
    command
        .args(["offers", "--no-config"])
        .env("VAST_URL", &base)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env_remove("VAST_API_KEY")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy");
    // A proxy that refuses every connection: a request sent through it cannot succeed.
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env(name, "http://127.0.0.1:1");
    }
    let out = command.output().unwrap();
    std::fs::remove_dir_all(&home).unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("5 offers"),
        "the table came back: {stderr}"
    );
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "the mock was reached directly"
    );
}

#[test]
fn a_closed_stdout_ends_an_offers_watch_quietly_instead_of_looping_on() {
    // `vas offers --watch 60 | head -1`: once the reader is gone the next write fails, and that is
    // the end, not a failure. A watch that treated the error as a write would loop for ever.
    let (base, _) = serve(bundles);
    let home = fresh_home("closed-pipe");
    let _cleanup = RemoveOnDrop(home.clone());
    let mut child = Command::new(env!("CARGO_BIN_EXE_vas"))
        .args(["offers", "--no-config", "--watch", "0"])
        .env("VAST_URL", &base)
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
    let mut first = String::new();
    std::io::BufRead::read_line(&mut std::io::BufReader::new(stdout), &mut first).unwrap();
    assert!(first.trim_start().starts_with("ID"), "{first}");
    // The reader is dropped at the end of the statement above: the pipe is closed.
    let status = common::exit_within_deadline(&mut child);
    assert_eq!(status.code(), Some(0), "a closed pipe is not a failure");
}

/// Unix seconds now.
fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

/// A `vas offers` child (no config, no key) with its stderr piped, its stdout `stdout`, and a home
/// that goes with the test.
fn offers_child(
    base: &str,
    name: &str,
    args: &[&str],
    floor_ms: Option<&str>,
    stdout: Stdio,
) -> (std::process::Child, RemoveOnDrop) {
    let home = fresh_home(name);
    let mut command = Command::new(env!("CARGO_BIN_EXE_vas"));
    command
        .args(["offers", "--no-config"])
        .args(args)
        .env("VAST_URL", base)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env_remove("VAST_API_KEY")
        .env_remove("XDG_CONFIG_HOME")
        .stdout(stdout)
        .stderr(Stdio::piped());
    match floor_ms {
        Some(ms) => command.env("VAS_WATCH_FLOOR_MS", ms),
        None => command.env_remove("VAS_WATCH_FLOOR_MS"),
    };
    (command.spawn().unwrap(), RemoveOnDrop(home))
}

/// The first search is refused for a rate limit that clears in 1 s; the next ones answer.
static LIMITED_ONCE: AtomicUsize = AtomicUsize::new(0);

fn rate_limited_once(_: &str) -> (u16, Vec<(String, String)>, String) {
    if LIMITED_ONCE.fetch_add(1, Ordering::SeqCst) == 0 {
        (
            429,
            vec![("Retry-After".to_owned(), "1".to_owned())],
            "{}".to_owned(),
        )
    } else {
        (200, Vec::new(), fixture("offers.json"))
    }
}

/// Every search refused, for two minutes.
fn rate_limited_long(_: &str) -> (u16, Vec<(String, String)>, String) {
    (
        429,
        vec![("Retry-After".to_owned(), "120".to_owned())],
        "{}".to_owned(),
    )
}

/// Every search answers, and says its budget (ONE call) is spent and renews in 3 s.
fn spent_budget(_: &str) -> (u16, Vec<(String, String)>, String) {
    let reset = format!("{:.0}", unix_now() + 3.0);
    (
        200,
        vec![
            ("X-RateLimit-Limit".to_owned(), "1".to_owned()),
            ("X-RateLimit-Remaining".to_owned(), "0".to_owned()),
            ("X-RateLimit-Reset".to_owned(), reset),
        ],
        fixture("offers.json"),
    )
}

#[test]
fn a_short_rate_limit_is_waited_out_once_and_said_aloud() {
    let (base, seen) = common::serve_with(rate_limited_once);
    let (mut child, _home) = offers_child(&base, "limited-once", &[], None, Stdio::piped());
    let status = common::exit_within_deadline(&mut child);
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
    let mut stdout = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut stdout).unwrap();
    assert_eq!(status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("vas offers: rate-limited: waiting 1 s, then trying again"),
        "{stderr}"
    );
    assert!(
        stdout.contains("5 offers"),
        "the retry's answer is shown: {stdout}"
    );
    let (first, second) = {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "one refusal, one retry");
        (seen[0].at, seen[1].at)
    };
    let gap = second.duration_since(first);
    assert!(
        gap >= std::time::Duration::from_millis(1500),
        "the 1 s named, and a margin: {gap:?}"
    );
}

#[test]
fn a_long_rate_limit_is_named_not_waited_out() {
    let (base, seen) = common::serve_with(rate_limited_long);
    let (mut child, _home) = offers_child(&base, "limited-long", &[], None, Stdio::null());
    // The deadline of the helper is 20 s: sitting out 120 s would fail here, not hang the suite.
    let status = common::exit_within_deadline(&mut child);
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
    assert_eq!(status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("vast.ai rate-limited /api/v0/bundles/: try again in 120 s"),
        "{stderr}"
    );
    assert_eq!(seen.lock().unwrap().len(), 1, "no second request");
}

#[test]
fn a_watch_leaves_vast_ai_alone_until_its_budget_renews() {
    let (base, seen) = common::serve_with(spent_budget);
    // `--watch 0` with the test floor of 50 ms would look again at once, were it not for the budget.
    let (mut child, _home) = offers_child(
        &base,
        "spent-budget",
        &["--watch", "0"],
        Some("50"),
        Stdio::piped(),
    );
    let started = std::time::Instant::now();
    while seen.lock().unwrap().len() < 2 {
        assert!(
            started.elapsed() < common::PROCESS_DEADLINE,
            "the second look never came"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
    let (first, second) = {
        let seen = seen.lock().unwrap();
        (seen[0].at, seen[1].at)
    };
    let gap = second.duration_since(first);
    assert!(
        gap >= std::time::Duration::from_millis(2500),
        "the budget renews in about 3 s (plus a margin), not in 50 ms: {gap:?}"
    );
    assert!(
        stderr.contains("vas offers: vast.ai's request budget renews in"),
        "the wait is said once, on stderr: {stderr}"
    );
}

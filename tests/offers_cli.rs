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

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
/// and no key in the environment; then the home is removed. Returns the output, stdout, stderr.
fn vas(base: &str, home: &Path, xdg: bool, args: &[&str]) -> (Output, String, String) {
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
    let out = command.output().unwrap();
    std::fs::remove_dir_all(home).unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (out, stdout, stderr)
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
    let mut command = Command::new(env!("CARGO_BIN_EXE_vas"));
    let found = command
        .args(["offers", "--config"])
        .arg(&file)
        .env("VAST_URL", &base)
        .env_remove("VAST_API_KEY")
        .output()
        .unwrap();
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
    assert!(stderr.contains("line 2: not a flag"), "{stderr}");
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

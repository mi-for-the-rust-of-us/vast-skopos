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

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{exit_within_deadline, read_lines_until, serve};

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

/// The calls of [`vas`] so far, to name each one's home.
static CALLS: AtomicUsize = AtomicUsize::new(0);

/// `vas args` against the mock, with a key in the environment and an empty home; stdout.
fn vas(args: &[&str]) -> String {
    let (base, _) = serve(account);
    // One home per call: the tests run in parallel, and each removes its own when it is done.
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let home = std::env::temp_dir().join(format!("vas-look-{call}-{}", std::process::id()));
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

#[test]
fn compact_prints_one_json_object_per_box_then_the_money_and_nothing_else() {
    let text = vas(&["--compact"]);
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect();
    let kinds: Vec<&str> = lines.iter().map(|l| l["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["box", "box", "box", "money"], "{text}");
    assert_eq!(lines[0]["id"], 56_362_434);
    assert_eq!(
        (
            lines[0]["machine_id"].as_u64(),
            lines[0]["host_id"].as_u64()
        ),
        (Some(100_682), Some(406_325)),
        "the ids are always there, `--wide` or not"
    );
    assert_eq!(
        (lines[0]["ssh_host"].as_str(), lines[0]["ssh_port"].as_u64()),
        (Some("ssh6.vast.ai"), Some(36_000))
    );
    assert_eq!(
        lines[1]["machine_id"], 140_734,
        "a numeric-string id is a number"
    );
    assert!(lines[2]["machine_id"].is_null() && lines[2]["probe"].is_null());
    assert_eq!(lines[3]["credit"], 12.93);
    assert_eq!(lines[3]["running"], 1);
    assert!(
        !text.contains("credit $"),
        "no table, no money line: {text}"
    );
}

#[test]
fn compact_and_wide_are_refused_together() {
    let (base, _) = serve(account);
    let out = Command::new(env!("CARGO_BIN_EXE_vas"))
        .args(["--compact", "--wide"])
        .env("VAST_URL", &base)
        .env("VAST_API_KEY", "k3y")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("cannot be used with"), "{stderr}");
}

/// `vas args` as a WATCH against `respond`, the 30 s floor set to `floor_ms` (none: the real one),
/// with piped stdout and stderr; the caller reads it and the home is left for the OS (a temp dir).
fn spawn_watch(respond: common::Respond, args: &[&str], floor_ms: Option<&str>) -> Watch {
    let (base, _) = serve(respond);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let home = std::env::temp_dir().join(format!("vas-look-watch-{call}-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_vas"));
    command
        .args(args)
        .env("VAST_URL", &base)
        .env("VAST_API_KEY", "k3y")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env_remove("XDG_CONFIG_HOME")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match floor_ms {
        Some(ms) => command.env("VAS_WATCH_FLOOR_MS", ms),
        None => command.env_remove("VAS_WATCH_FLOOR_MS"),
    };
    Watch {
        child: command.spawn().unwrap(),
        home,
    }
}

/// A watching `vas` and the home it was given: killed, waited for and removed when the test ends,
/// even when an assertion panics first (a looping child must not outlive its test).
struct Watch {
    child: Child,
    home: PathBuf,
}

impl std::ops::Deref for Watch {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}

impl std::ops::DerefMut for Watch {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// Every request refused: a key vast.ai does not know.
fn refused(_: &str) -> (u16, String) {
    (401, "{}".to_owned())
}

#[test]
fn a_compact_watch_prints_json_lines_only_and_each_look_carries_its_clock() {
    let mut child = spawn_watch(account, &["--compact", "--watch", "0"], Some("50"));
    // Two looks: each is three boxes and the money.
    let lines = read_lines_until(&mut child, "\"kind\":\"money\"", 2);
    let objects: Vec<serde_json::Value> = lines
        .iter()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l:?} in {lines:#?}")))
        .collect();
    assert!(objects.len() >= 8, "{lines:#?}");
    assert!(
        lines.iter().all(|l| !l.starts_with("==")),
        "no stamp line between looks, which would not parse: {lines:#?}"
    );
    let clocks: Vec<u64> = objects.iter().map(|o| o["at"].as_u64().unwrap()).collect();
    assert!(
        clocks.windows(2).all(|w| w[0] <= w[1]),
        "`at` never goes back: {clocks:?}"
    );
    // The looks are told apart by the money line closing each of them.
    let money = objects.iter().filter(|o| o["kind"] == "money").count();
    assert_eq!(money, 2, "{lines:#?}");
}

#[test]
fn a_table_watch_stamps_each_look() {
    let mut child = spawn_watch(account, &["--watch", "0"], Some("50"));
    let lines = read_lines_until(&mut child, "== vas, unix time", 2);
    assert!(
        lines[0].starts_with("== vas, unix time "),
        "a stamp opens the first look: {lines:#?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("credit $12.93")),
        "{lines:#?}"
    );
}

#[test]
fn a_watch_whose_first_look_is_refused_gives_up_instead_of_looping_on_stderr() {
    let mut child = spawn_watch(refused, &["--watch", "0"], Some("50"));
    let status = exit_within_deadline(&mut child);
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
    assert_eq!(status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("status 401"), "{stderr}");
    assert_eq!(
        stderr.matches("status 401").count(),
        1,
        "said once, not on every look: {stderr}"
    );
}

#[test]
fn a_watch_shorter_than_the_floor_is_raised_and_says_so() {
    // The real floor (30 s): the first look is immediate, and the raise is said on stderr.
    let mut child = spawn_watch(account, &["--compact", "--watch", "1"], None);
    let _ = read_lines_until(&mut child, "\"kind\":\"money\"", 1);
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
    assert!(
        stderr.contains("vas: --watch 1 is raised to 30 s"),
        "{stderr}"
    );
    // And with the floor of the tests (50 ms, below the 1 s asked) nothing is raised.
    let mut child = spawn_watch(account, &["--compact", "--watch", "1"], Some("50"));
    let _ = read_lines_until(&mut child, "\"kind\":\"money\"", 1);
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
    assert!(!stderr.contains("raised"), "{stderr}");
}

/// What a run of `vas` against the mock left: the exit code, the output, and the requests seen.
struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    seen: common::Seen,
}

/// `vas args` against the mock, with NO key in the environment but what `env` adds, and the
/// variables in `unset` removed; no home of its own (the caller sets `HOME` and `USERPROFILE`).
fn vas_with(
    respond: common::Respond,
    args: &[&str],
    env: &[(&str, &std::ffi::OsStr)],
    unset: &[&str],
) -> Run {
    vas_in(respond, args, env, unset, None)
}

/// [`vas_with`], run from `cwd` when given (a relative path in a lookup then names a file there).
fn vas_in(
    respond: common::Respond,
    args: &[&str],
    env: &[(&str, &std::ffi::OsStr)],
    unset: &[&str],
    cwd: Option<&Path>,
) -> Run {
    let (base, seen) = serve(respond);
    let mut command = Command::new(env!("CARGO_BIN_EXE_vas"));
    command
        .args(args)
        .env("VAST_URL", &base)
        .env_remove("VAST_API_KEY")
        .env_remove("XDG_CONFIG_HOME");
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    for name in unset {
        command.env_remove(name);
    }
    for (name, value) in env {
        command.env(name, value);
    }
    let out = command.output().unwrap();
    Run {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        seen,
    }
}

/// A fresh directory holding a `.vast_api_key` of `key`; the caller removes it.
fn home_with_key(name: &str, key: &str) -> std::path::PathBuf {
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("vas-home-{name}-{call}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".vast_api_key"), key).unwrap();
    dir
}

#[test]
fn the_home_is_found_the_way_the_official_cli_finds_it() {
    let key = "filekey1234567890";
    let real = home_with_key("real", key);
    let bogus = std::env::temp_dir().join("vas-home-that-does-not-exist");
    // The variable the platform looks at FIRST: USERPROFILE on Windows (where HOME is often an MSYS
    // path no Windows call understands), HOME elsewhere.
    let (first, second) = if cfg!(windows) {
        ("USERPROFILE", "HOME")
    } else {
        ("HOME", "USERPROFILE")
    };
    let found = vas_with(
        account,
        &[],
        &[(first, real.as_os_str()), (second, bogus.as_os_str())],
        &[],
    );
    assert_eq!(found.code, Some(0), "{}", found.stderr);
    assert_eq!(
        found.seen.lock().unwrap()[0].authorization.as_deref(),
        Some("Bearer filekey1234567890"),
        "the key of the home the platform prefers is the one sent"
    );
    // Swapped, the preferred variable names nowhere: the other is only a fallback.
    let lost = vas_with(
        account,
        &[],
        &[(second, real.as_os_str()), (first, bogus.as_os_str())],
        &[],
    );
    assert_eq!(lost.code, Some(1), "{}", lost.stdout);
    assert!(
        lost.stderr.contains("no vast.ai API key found"),
        "{}",
        lost.stderr
    );
    assert!(
        lost.seen.lock().unwrap().is_empty(),
        "no request without a key"
    );
    // Only the fallback set (the other variable unset): it serves.
    let fallback = vas_with(account, &[], &[(second, real.as_os_str())], &[first]);
    assert_eq!(fallback.code, Some(0), "{}", fallback.stderr);
    std::fs::remove_dir_all(&real).unwrap();
}

/// A key as `ApiKey::new` accepts it.
const ENV_KEY: &str = "envkey1234567890";

/// The environment of a run with a key and an empty home, as `vas_with` takes it.
fn key_env<'a>(home: &'a Path, key: &'a str) -> Vec<(&'a str, &'a std::ffi::OsStr)> {
    vec![
        ("VAST_API_KEY", std::ffi::OsStr::new(key)),
        ("HOME", home.as_os_str()),
        ("USERPROFILE", home.as_os_str()),
    ]
}

/// The home variables of an empty `home`, and `extra`.
fn env_with<'a>(
    home: &'a Path,
    extra: &[(&'a str, &'a std::ffi::OsStr)],
) -> Vec<(&'a str, &'a std::ffi::OsStr)> {
    let mut env = vec![
        ("HOME", home.as_os_str()),
        ("USERPROFILE", home.as_os_str()),
    ];
    env.extend_from_slice(extra);
    env
}

/// A fresh empty directory; the caller removes it.
fn empty_dir(name: &str) -> PathBuf {
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("vas-dir-{name}-{call}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes `key` at `dir/relative`, creating the folders.
fn write_key(dir: &Path, relative: &str, key: &str) {
    let file = dir.join(relative);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, key).unwrap();
}

/// The `Authorization` header of the first request the mock saw.
fn bearer(run: &Run) -> Option<String> {
    run.seen
        .lock()
        .unwrap()
        .first()
        .and_then(|r| r.authorization.clone())
}

/// The instances answer, the account refused with a 500.
fn credit_refused(target: &str) -> (u16, String) {
    if target.starts_with("/api/v0/users/current") {
        (500, "{}".to_owned())
    } else {
        account(target)
    }
}

/// One running box whose `ssh` endpoint is this machine's discard port: a probe is refused at once.
fn box_on_loopback(target: &str) -> (u16, String) {
    if target.starts_with("/api/v1/instances/") {
        (
            200,
            r#"{"instances": [{"id": 77, "actual_status": "running", "dph_total": 0.5,
                "start_date": 1.0, "ssh_host": "127.0.0.1", "ssh_port": 9}], "next_token": null}"#
                .to_owned(),
        )
    } else {
        account(target)
    }
}

#[test]
fn a_refused_account_leaves_the_table_without_credit_and_says_so() {
    let home = empty_dir("credit");
    let run = vas_with(credit_refused, &[], &key_env(&home, ENV_KEY), &[]);
    std::fs::remove_dir_all(&home).unwrap();
    assert_eq!(
        run.code,
        Some(0),
        "the boxes are still worth showing: {}",
        run.stderr
    );
    assert!(
        run.stderr.contains("vas: no credit shown: "),
        "{}",
        run.stderr
    );
    assert!(
        run.stdout.contains("56362434"),
        "the table is there: {}",
        run.stdout
    );
    assert!(
        run.stdout
            .contains("credit ? | burn $0.900/h over 1 running"),
        "{}",
        run.stdout
    );
}

#[test]
fn ssh_probes_the_running_boxes_and_shows_what_it_could_not_read() {
    let home = empty_dir("ssh");
    let env = key_env(&home, ENV_KEY);
    // Nothing listens on the discard port: the probe fails at once, and the row says where and why.
    let table = vas_with(box_on_loopback, &["--ssh"], &env, &[]);
    assert_eq!(table.code, Some(0), "{}", table.stderr);
    let header = table.stdout.lines().next().unwrap();
    assert!(
        header.contains("GPU% (ssh)") && header.contains("LAST LOG / SSH ERROR"),
        "{header}"
    );
    let row = table.stdout.lines().find(|l| l.starts_with("77")).unwrap();
    assert!(
        row.contains("failed to probe 127.0.0.1:9 over ssh"),
        "{row}"
    );
    // The same, as JSON: the probe is an object with an `error`, not null.
    let json = vas_with(box_on_loopback, &["--compact", "--ssh"], &env, &[]);
    std::fs::remove_dir_all(&home).unwrap();
    assert_eq!(json.code, Some(0), "{}", json.stderr);
    let first: serde_json::Value =
        serde_json::from_str(json.stdout.lines().next().unwrap()).unwrap();
    assert!(
        first["probe"]["error"]
            .as_str()
            .is_some_and(|e| e.contains("failed to probe 127.0.0.1:9")),
        "{first}"
    );
    // Without --ssh nothing is probed: the probe is null.
    let home = empty_dir("nossh");
    let quiet = vas_with(
        box_on_loopback,
        &["--compact"],
        &key_env(&home, ENV_KEY),
        &[],
    );
    std::fs::remove_dir_all(&home).unwrap();
    let first: serde_json::Value =
        serde_json::from_str(quiet.stdout.lines().next().unwrap()).unwrap();
    assert!(first["probe"].is_null(), "{first}");
}

#[test]
fn the_key_is_found_where_the_official_cli_saves_it_and_the_first_place_wins() {
    let home = empty_dir("keys-home");
    let xdg = empty_dir("keys-xdg");
    write_key(&home, ".config/vastai/vast_api_key", "cfgkey1234567890");
    // ~/.config/vastai/vast_api_key.
    let run = vas_with(account, &[], &env_with(&home, &[]), &[]);
    assert_eq!(
        bearer(&run).as_deref(),
        Some("Bearer cfgkey1234567890"),
        "{}",
        run.stderr
    );
    // An EMPTY XDG_CONFIG_HOME is no directory. Unfiltered it would read as the RELATIVE path
    // `vastai/vast_api_key`, so a bait file there, in the folder `vas` runs from, tells the two apart.
    let bait = empty_dir("keys-bait");
    write_key(&bait, "vastai/vast_api_key", "baitkey1234567890");
    let run = vas_in(
        account,
        &[],
        &env_with(&home, &[("XDG_CONFIG_HOME", std::ffi::OsStr::new(""))]),
        &[],
        Some(&bait),
    );
    std::fs::remove_dir_all(&bait).unwrap();
    assert_eq!(
        bearer(&run).as_deref(),
        Some("Bearer cfgkey1234567890"),
        "the empty variable must not make a relative path: {}",
        run.stderr
    );
    // And the plain case: the home's file is found.
    let run = vas_with(
        account,
        &[],
        &env_with(&home, &[("XDG_CONFIG_HOME", std::ffi::OsStr::new(""))]),
        &[],
    );
    assert_eq!(
        bearer(&run).as_deref(),
        Some("Bearer cfgkey1234567890"),
        "{}",
        run.stderr
    );
    // $XDG_CONFIG_HOME/vastai/vast_api_key comes before the home's.
    write_key(&xdg, "vastai/vast_api_key", "xdgkey1234567890");
    let run = vas_with(
        account,
        &[],
        &env_with(&home, &[("XDG_CONFIG_HOME", xdg.as_os_str())]),
        &[],
    );
    assert_eq!(
        bearer(&run).as_deref(),
        Some("Bearer xdgkey1234567890"),
        "{}",
        run.stderr
    );
    // VAST_API_KEY comes before every file.
    let run = vas_with(
        account,
        &[],
        &env_with(
            &home,
            &[
                ("XDG_CONFIG_HOME", xdg.as_os_str()),
                ("VAST_API_KEY", std::ffi::OsStr::new(ENV_KEY)),
            ],
        ),
        &[],
    );
    assert_eq!(
        bearer(&run).as_deref(),
        Some("Bearer envkey1234567890"),
        "{}",
        run.stderr
    );
    // --key-file comes before the environment.
    let file = xdg.join("chosen");
    std::fs::write(&file, "chosen1234567890").unwrap();
    let chosen = file.to_string_lossy().into_owned();
    let run = vas_with(
        account,
        &["--key-file", &chosen],
        &env_with(&home, &[("VAST_API_KEY", std::ffi::OsStr::new(ENV_KEY))]),
        &[],
    );
    assert_eq!(
        bearer(&run).as_deref(),
        Some("Bearer chosen1234567890"),
        "{}",
        run.stderr
    );
    std::fs::remove_dir_all(&home).unwrap();
    std::fs::remove_dir_all(&xdg).unwrap();
}

#[test]
fn a_base_url_that_is_not_the_default_is_said_aloud_with_where_the_key_goes() {
    let home = empty_dir("notice");
    let run = vas_with(account, &[], &key_env(&home, ENV_KEY), &[]);
    std::fs::remove_dir_all(&home).unwrap();
    assert!(
        run.stderr
            .contains("vas: VAST_URL is set: the API key is sent to http://127.0.0.1")
            && run.stderr.contains(", NOT over https"),
        "the key goes to a plain-http address: {}",
        run.stderr
    );
}

#[test]
fn flags_that_make_no_sense_alone_are_refused_before_any_request() {
    let home = empty_dir("flags");
    let env = key_env(&home, ENV_KEY);
    let lonely = vas_with(account, &["--log", "/root/x.log"], &env, &[]);
    assert_eq!(lonely.code, Some(2), "--log needs --ssh: {}", lonely.stderr);
    assert!(lonely.stderr.contains("--ssh"), "{}", lonely.stderr);
    let blank = vas_with(account, &["--ssh", "--log", "  "], &env, &[]);
    std::fs::remove_dir_all(&home).unwrap();
    assert_eq!(blank.code, Some(2), "{}", blank.stderr);
    assert!(
        blank.stderr.contains("blank"),
        "an unset variable must not list `.`: {}",
        blank.stderr
    );
    assert!(lonely.seen.lock().unwrap().is_empty() && blank.seen.lock().unwrap().is_empty());
}

#[test]
fn a_closed_stdout_ends_a_watch_quietly_instead_of_looping_on() {
    // `vas --watch 60 | head -1`: once the reader is gone, the next write fails, and that is the end.
    let mut child = spawn_watch(account, &["--compact", "--watch", "0"], Some("50"));
    let stdout = child.stdout.take().unwrap();
    let mut first = String::new();
    std::io::BufReader::new(stdout)
        .read_line(&mut first)
        .unwrap();
    assert!(first.starts_with('{'), "{first}");
    // The reader (stdout, here consumed by the BufReader) is dropped: the pipe is closed.
    let status = exit_within_deadline(&mut child);
    assert_eq!(status.code(), Some(0), "a closed pipe is not a failure");
}

#[test]
fn a_key_file_may_be_utf8_with_a_bom_or_utf16_of_either_endian() {
    // Notepad and PowerShell write these; the key must come out the same.
    let key = "uniKey1234567890";
    let mut be = vec![0xFE, 0xFF];
    be.extend(key.encode_utf16().flat_map(u16::to_be_bytes));
    let mut le = vec![0xFF, 0xFE];
    le.extend(key.encode_utf16().flat_map(u16::to_le_bytes));
    let mut bom8 = vec![0xEF, 0xBB, 0xBF];
    bom8.extend_from_slice(key.as_bytes());
    for (name, bytes) in [("utf16-be", be), ("utf16-le", le), ("utf8-bom", bom8)] {
        let home = empty_dir(name);
        std::fs::write(home.join(".vast_api_key"), bytes).unwrap();
        let run = vas_with(account, &[], &env_with(&home, &[]), &[]);
        std::fs::remove_dir_all(&home).unwrap();
        assert_eq!(
            bearer(&run).as_deref(),
            Some("Bearer uniKey1234567890"),
            "{name}: {}",
            run.stderr
        );
    }
}

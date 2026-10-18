//! The JSON Lines of `vas --compact`: one parseable object per box, then the money's; the same
//! keys on every box, `null` for what is not known, one line per object whatever the text holds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

use serde_json::{Value, json};
use vast_skopos::SkoposError;
use vast_skopos::compact::json_lines;
use vast_skopos::instance::{Account, Instance};
use vast_skopos::probe::parse_output;

fn instance(row: &Value) -> Instance {
    Instance::from_json(row).unwrap()
}

/// The lines of `text`, each parsed: a line that is not JSON fails the test.
fn objects(text: &str) -> Vec<Value> {
    assert!(text.ends_with('\n'), "every line ends in a newline");
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

/// The keys of an object, sorted.
fn keys(v: &Value) -> Vec<&str> {
    let mut k: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    k.sort_unstable();
    k
}

#[test]
fn a_box_with_everything_and_a_box_with_nothing_have_the_same_keys() {
    let full = instance(
        &json!({"id": 56_362_434, "label": "me36 box 2", "gpu_name": "RTX 5090",
        "num_gpus": 1, "geolocation": "South Korea, KR", "actual_status": "running",
        "intended_status": "running", "dph_total": 0.9, "start_date": 1_000.0,
        "ssh_host": "ssh6.vast.ai", "ssh_port": 36_000, "machine_id": 100_682, "host_id": 406_325}),
    );
    let bare = instance(&json!({"id": 2}));
    let probes = vec![
        Some(Ok(parse_output(
            "0, 99, 29400, 32607\n---vas-log---\nvas-file:/root/runs/s0/train.log\ne41 loss 0.3\n",
        ))),
        None,
    ];
    let lines = objects(&json_lines(&[full, bare], &probes, None, 1_000.0 + 8_100.0));
    assert_eq!(lines.len(), 3, "two boxes and the money");
    assert_eq!(
        keys(&lines[0]),
        keys(&lines[1]),
        "the same keys, whatever is known"
    );
    let b = &lines[0];
    assert_eq!(b["kind"], "box");
    assert_eq!(b["at"], 9_100);
    assert_eq!(
        (b["id"].as_u64(), b["label"].as_str()),
        (Some(56_362_434), Some("me36 box 2"))
    );
    assert_eq!(
        (b["gpu_name"].as_str(), b["num_gpus"].as_u64()),
        (Some("RTX 5090"), Some(1))
    );
    assert_eq!(
        (b["status"].as_str(), b["intended"].as_str()),
        (Some("running"), Some("running"))
    );
    assert_eq!(
        (b["age_s"].as_u64(), b["cost"].as_f64()),
        (Some(8_100), Some(2.025))
    );
    assert_eq!(
        (b["ssh_host"].as_str(), b["ssh_port"].as_u64()),
        (Some("ssh6.vast.ai"), Some(36_000))
    );
    assert_eq!(
        (b["machine_id"].as_u64(), b["host_id"].as_u64()),
        (Some(100_682), Some(406_325))
    );
    assert_eq!(
        b["probe"]["gpus"],
        json!([{"index": 0, "util_pct": 99, "mem_used_mib": 29_400, "mem_total_mib": 32_607}])
    );
    assert_eq!(
        b["probe"]["log"],
        json!({"state": "line", "file": "/root/runs/s0/train.log", "line": "e41 loss 0.3"})
    );
    let n = &lines[1];
    assert_eq!(n["id"], 2);
    for unknown in [
        "label",
        "gpu_name",
        "num_gpus",
        "where",
        "status",
        "intended",
        "dph",
        "age_s",
        "cost",
        "ssh_host",
        "ssh_port",
        "machine_id",
        "host_id",
        "probe",
    ] {
        assert!(n[unknown].is_null(), "{unknown} is null, not missing: {n}");
    }
}

#[test]
fn a_failed_probe_and_every_log_state_are_objects() {
    let rows: Vec<Instance> = (1..=6)
        .map(|id| instance(&json!({"id": id, "actual_status": "running"})))
        .collect();
    let log = |tail: &str| {
        Some(Ok(parse_output(&format!(
            "0, 0, 0, 32607\n---vas-log---\n{tail}"
        ))))
    };
    let probes = vec![
        Some(Err(SkoposError::Ssh {
            target: "h:1".to_owned(),
            message: "Connection refused\nsecond line, with a \u{1b}[2J".to_owned(),
        })),
        log("vas-no-file\n"),
        log("vas-not-a-file:/root/runs/s1\n"),
        log("vas-file:/root/runs/s1/train.log\n"),
        log("vas-file:/root/runs/s1/train.log\nlast\n"),
        log("garbage\n"),
    ];
    let text = json_lines(&rows, &probes, None, 0.0);
    assert_eq!(
        text.lines().count(),
        7,
        "one line per object, whatever the text holds"
    );
    let lines = objects(&text);
    let msg = lines[0]["probe"]["error"].as_str().unwrap();
    assert!(
        msg.contains("Connection refused") && msg.contains("second line"),
        "{msg}"
    );
    assert!(
        lines[0]["probe"].get("gpus").is_none(),
        "an error has no readings"
    );
    let states: Vec<&str> = lines[1..6]
        .iter()
        .map(|l| l["probe"]["log"]["state"].as_str().unwrap())
        .collect();
    assert_eq!(
        states,
        ["no_match", "not_a_file", "empty", "line", "unreadable"]
    );
    assert_eq!(lines[2]["probe"]["log"]["file"], "/root/runs/s1");
}

#[test]
fn the_money_carries_the_unsure_boxes_by_state_and_rounds_the_float_noise() {
    let row = |id: u64, actual: &str, intended: &str, dph: f64| {
        instance(
            &json!({"id": id, "actual_status": actual, "intended_status": intended,
            "dph_total": dph, "start_date": 0.0}),
        )
    };
    let fleet = [
        row(1, "running", "running", 0.1),
        row(2, "running", "running", 0.2),
        row(3, "offline", "running", 0.3),
        row(4, "exited", "stopped", 5.0),
    ];
    let credit = Account::from_json(&json!({"credit": 12.0}));
    let lines = objects(&json_lines(&fleet, &[], Some(&credit), 3_600.0));
    let money = lines.last().unwrap();
    assert_eq!(money["kind"], "money");
    assert_eq!(money["at"], 3_600);
    assert_eq!(money["credit"], 12.0);
    assert_eq!(
        money["burn"], 0.6,
        "0.1 + 0.2 + 0.3 is not 0.6000000000000001 here"
    );
    assert_eq!(money["running"], 2);
    assert_eq!(money["unsure"], json!({"offline": 1}));
    assert_eq!(money["hours_left"], 20.0);
    // vast.ai's own credit carries float noise (seen live: 12.059765171599793); ten-thousandths are kept.
    let noisy = Account::from_json(&json!({"credit": 12.059_765_171_599_793}));
    let lines = objects(&json_lines(&fleet, &[], Some(&noisy), 3_600.0));
    assert_eq!(lines.last().unwrap()["credit"], 12.0598);
    // The same keys with nothing known: a monitor reads one schema.
    let empty = objects(&json_lines(&[], &[], None, 0.0));
    assert_eq!(empty.len(), 1, "no boxes: the money alone");
    assert_eq!(keys(&empty[0]), keys(money));
    assert!(empty[0]["credit"].is_null() && empty[0]["hours_left"].is_null());
    assert_eq!(empty[0]["unsure"], json!({}));
}

#[test]
fn money_that_rounds_to_zero_is_zero_not_minus_zero() {
    // A sliver of owed money: -0.00001 rounds to `0.0`, and `-0.0` would be printed otherwise.
    let owed = Account::from_json(&json!({"credit": -0.000_01}));
    let text = json_lines(&[], &[], Some(&owed), 0.0);
    assert!(text.contains("\"credit\":0.0"), "{text}");
    assert!(!text.contains("-0.0"), "{text}");
    // A burn too small to give a runway: `hours_left` is null, as the text line leaves it out.
    let tiny = instance(&json!({"id": 1, "actual_status": "running", "dph_total": 1e-320}));
    let credit = Account::from_json(&json!({"credit": 5.0}));
    let lines = objects(&json_lines(&[tiny], &[], Some(&credit), 0.0));
    assert!(lines.last().unwrap()["hours_left"].is_null());
}

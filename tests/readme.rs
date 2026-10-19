//! The README's examples against what the code prints: a sample that names a column or a key the
//! program no longer has (or lacks one it has) is a lie the reader will believe.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
// EXPLICIT: test code -- a failed unwrap or expect IS the failed assertion.

use std::collections::BTreeSet;

use vast_skopos::compact::json_lines;
use vast_skopos::instance::Instance;
use vast_skopos::probe::{ProbeOutcome, parse_output};
use vast_skopos::render::{TableOptions, table};

/// The README, with LF line ends whatever the checkout's.
fn readme() -> String {
    include_str!("../README.md").replace("\r\n", "\n")
}

/// A probe with every section the remote command prints, for a GPU capped at 400 W.
const FULL: &str = "0, 100, 24809, 32607\n---vas-power---\n0, 399.9, 400, 575, 1695, 3090, 0x4\n\
                    ---vas-apps---\n0, 00000000:C1:00.0\n---vas-apps-by-gpu---\n00000000:C1:00.0, 8720\nvas-apps-ok\n\
                    ---vas-log---\nvas-file:/r/s0/train.log\nepoch 41\n";

fn running() -> Instance {
    Instance::from_json(&serde_json::json!({"id": 1, "actual_status": "running"})).unwrap()
}

fn probed() -> [ProbeOutcome; 1] {
    [Some(Ok(parse_output(FULL)))]
}

/// The column names of a table's header line: names are parted by two spaces or more.
fn columns(header: &str) -> Vec<String> {
    header
        .split("  ")
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

fn keys(value: &serde_json::Value) -> BTreeSet<String> {
    value.as_object().unwrap().keys().cloned().collect()
}

#[test]
fn the_try_it_sample_has_the_columns_the_table_has() {
    let readme = readme();
    let at = readme
        .find("$ vas --ssh\n")
        .expect("the sample is in the README");
    let sample_header = readme[at..].lines().nth(1).unwrap();
    let real = table(
        &[running()],
        &probed(),
        None,
        TableOptions::new().ssh(true),
        0.0,
    );
    assert_eq!(
        columns(sample_header),
        columns(real.lines().next().unwrap()),
        "regenerate the `Try it` sample"
    );
}

#[test]
fn the_probe_columns_are_named_in_the_order_the_table_prints_them() {
    let real = table(
        &[running()],
        &probed(),
        None,
        TableOptions::new().ssh(true),
        0.0,
    );
    let header = columns(real.lines().next().unwrap());
    let from = header.iter().position(|c| c == "GPU% (ssh)").unwrap();
    // The README names the columns up to the last one, `LAST LOG / SSH ERROR`, which it describes
    // apart (it is the log's, not a reading of the GPU).
    let probe_columns = &header[from..];
    let said = format!(
        "in order: `{}`",
        probe_columns[..probe_columns.len() - 1].join("`, `")
    );
    assert!(
        readme().contains(&said),
        "the README names the probe columns as `{said}`"
    );
}

#[test]
fn the_compact_example_has_the_keys_the_json_has() {
    let readme = readme();
    let line = readme
        .lines()
        .find(|l| l.starts_with("{\"age_s\""))
        .expect("the example is in the README");
    let example: serde_json::Value = serde_json::from_str(line).unwrap();
    let out = json_lines(&[running()], &probed(), None, None, 0.0);
    let real: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    assert_eq!(keys(&example), keys(&real), "the box object");
    assert_eq!(
        keys(&example["probe"]),
        keys(&real["probe"]),
        "the probe object"
    );
    assert_eq!(
        keys(&example["probe"]["gpus"][0]),
        keys(&real["probe"]["gpus"][0]),
        "the GPU object"
    );
    assert_eq!(
        keys(&example["probe"]["log"]),
        keys(&real["probe"]["log"]),
        "the log object"
    );
    // The README names the three kinds (the code prints `box` and `money` here, and `event` under
    // `--ssh --watch`, which tests/power.rs and tests/other.rs check).
    for kind in ["box", "event", "money"] {
        assert!(readme.contains(&format!("`{kind}`")), "{kind}");
    }
}

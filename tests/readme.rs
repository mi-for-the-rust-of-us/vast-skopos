//! The examples of the README and of `docs/reference.md` against what the code prints: a sample
//! that names a column or a key the program no longer has (or lacks one it has) is a lie the reader
//! will believe.

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

/// The reference page (every flag, column and key), with LF line ends.
fn reference() -> String {
    include_str!("../docs/reference.md").replace("\r\n", "\n")
}

/// The skill for agents (`vas --skill` prints it), with LF line ends.
fn skill() -> String {
    include_str!("../skills/vas/SKILL.md").replace("\r\n", "\n")
}

/// A probe for a GPU capped at 400 W: the utilisation, the power (its clock event reasons as a seventh
/// field, which the parser also reads), the processes and a log; no section of the board's maximum.
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
fn the_quick_start_s_ssh_sample_has_the_columns_the_table_has() {
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
        "regenerate the quick start's `vas --ssh` sample"
    );
}

#[test]
fn the_quick_start_s_plain_sample_has_the_columns_the_table_has() {
    let readme = readme();
    let at = readme
        .find("$ vas\n")
        .expect("the plain sample is in the README");
    let sample_header = readme[at..].lines().nth(1).unwrap();
    let real = table(&[running()], &[None], None, TableOptions::new(), 0.0);
    assert_eq!(
        columns(sample_header),
        columns(real.lines().next().unwrap()),
        "regenerate the quick start's `vas` sample"
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
    // The reference names the columns up to the last one, `LAST LOG / SSH ERROR`, which it describes
    // apart (it is the log's, not a reading of the GPU).
    let probe_columns = &header[from..];
    let said = format!(
        "in order: `{}`",
        probe_columns[..probe_columns.len() - 1].join("`, `")
    );
    assert!(
        reference().contains(&said),
        "the reference names the probe columns as `{said}`"
    );
}

#[test]
fn the_compact_example_has_the_keys_the_json_has() {
    let page = reference();
    let line = page
        .lines()
        .find(|l| l.starts_with("{\"age_s\""))
        .expect("the example is in the reference");
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
    let money_example: serde_json::Value = serde_json::from_str(
        page.lines()
            .find(|l| l.starts_with("{\"at\"") && l.contains("\"kind\":\"money\""))
            .expect("the money example is in the reference"),
    )
    .unwrap();
    let real_money: serde_json::Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
    assert_eq!(real_money["kind"], "money");
    assert_eq!(keys(&money_example), keys(&real_money), "the money object");
    // The reference names the three kinds (the code prints `box` and `money` here, and `event` under
    // `--ssh --watch`, which tests/power.rs and tests/other.rs check).
    for kind in ["box", "event", "money"] {
        assert!(page.contains(&format!("`{kind}`")), "{kind}");
    }
}

/// Every key of every object in `value`, at any depth, added to `keys`.
fn all_keys(value: &serde_json::Value, keys: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, inner) in map {
                keys.insert(key.clone());
                all_keys(inner, keys);
            }
        }
        serde_json::Value::Array(items) => {
            for inner in items {
                all_keys(inner, keys);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

#[test]
fn every_compact_key_the_skill_names_is_one_vas_writes() {
    use vast_skopos::trend::Trend;
    // Two looks of a watch: the limit falls from 575 to 400 W (an `event`), and the second look has
    // a box whose probe failed (the error object).
    let uncapped = FULL.replace("399.9, 400, 575", "560.0, 575, 575");
    let failed = vast_skopos::SkoposError::Ssh {
        target: String::from("h:1"),
        message: String::from("Connection refused"),
    };
    let mut trend = Trend::default();
    trend.record(&[running()], &[Some(Ok(parse_output(&uncapped)))]);
    let second = [
        running(),
        Instance::from_json(&serde_json::json!({"id": 2})).unwrap(),
    ];
    let probes = [Some(Ok(parse_output(FULL))), Some(Err(failed))];
    trend.record(&second, &probes);
    let out = json_lines(&second, &probes, Some(&trend), None, 0.0);
    assert!(out.contains("\"kind\":\"event\""), "{out}");
    let mut written = BTreeSet::new();
    for line in out.lines() {
        all_keys(&serde_json::from_str(line).unwrap(), &mut written);
    }
    let skill = skill();
    let region = skill
        .split_once("<!-- keys:")
        .and_then(|(_, rest)| rest.split_once("<!-- /keys -->"))
        .map(|(region, _)| region)
        .unwrap();
    // The opening comment's own line is prose, not keys.
    let (_, list) = region.split_once('\n').unwrap();
    let named: Vec<&str> = list.split('`').skip(1).step_by(2).collect();
    assert!(named.len() >= 50, "{named:?}");
    for key in &named {
        assert!(written.contains(*key), "--compact writes no key `{key}`");
    }
    // The empty test: a key the code does not write is caught.
    assert!(!written.contains("gpu_util"));
}

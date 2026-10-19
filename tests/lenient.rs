//! What the readers do with text and numbers they do not trust: characters that hide or reorder what
//! is shown, and numbers written in the other way a serialiser may write them.

#![allow(clippy::unwrap_used, clippy::unnecessary_wraps)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion; and `outcome` returns the
// `ProbeOutcome` (an `Option`) that the API takes.

use vast_skopos::compact::json_lines;
use vast_skopos::instance::Instance;
use vast_skopos::probe::{LogReading, ProbeOutcome, clean, parse_output, strip};
use vast_skopos::render::{TableOptions, table};

/// Characters that must never reach a terminal from a box: a right-to-left override, a zero-width
/// space, a soft hyphen, a deprecated format character, a variation selector, a tag character (which
/// can spell a hidden message), an interlinear annotation, and the byte-order mark.
const HIDDEN: [char; 8] = [
    '\u{202E}',
    '\u{200B}',
    '\u{00AD}',
    '\u{206A}',
    '\u{FE0F}',
    '\u{E0041}',
    '\u{FFF9}',
    '\u{FEFF}',
];

fn running(label: &str) -> Instance {
    Instance::from_json(&serde_json::json!({"id": 1, "actual_status": "running", "label": label}))
        .unwrap()
}

fn outcome(text: &str) -> ProbeOutcome {
    Some(Ok(parse_output(text)))
}

#[test]
fn a_log_line_and_a_file_name_lose_every_hidden_character_not_only_the_control_ones() {
    for hidden in HIDDEN {
        let line = format!("epoch {hidden}41 {hidden}done");
        assert_eq!(strip(&line), "epoch 41 done", "{hidden:?}");
        assert_eq!(clean(&line), "epoch 41 done", "{hidden:?}");
        let probe = parse_output(&format!(
            "0, 1, 1, 2\n---vas-log---\nvas-file:/r/a{hidden}.log\nhello {hidden}world\n"
        ));
        assert_eq!(
            probe.log,
            Some(LogReading::Line {
                file: "/r/a.log".to_owned(),
                line: "hello world".to_owned()
            }),
            "{hidden:?}"
        );
    }
    // Ordinary text, accents and CJK are not touched.
    assert_eq!(strip("é 日本語 ok"), "é 日本語 ok");
}

#[test]
fn what_reaches_the_table_and_the_json_from_a_probe_is_clean() {
    let output = "0, 1, 1, 2\n---vas-log---\nvas-file:/r/s\u{202E}0/train.log\ne\u{200B}poch 41\n";
    let text = table(
        &[running("box")],
        &[outcome(output)],
        None,
        TableOptions::new().ssh(true),
        0.0,
    );
    assert!(text.contains("s0/train.log: epoch 41"), "{text}");
    for hidden in HIDDEN {
        assert!(!text.contains(hidden), "{hidden:?} in {text}");
    }
    let json = json_lines(&[running("box")], &[outcome(output)], None, None, 0.0);
    for hidden in HIDDEN {
        assert!(!json.contains(hidden), "{hidden:?} in {json}");
    }
}

#[test]
fn the_api_text_loses_the_same_characters() {
    for hidden in HIDDEN {
        let label = format!("me{hidden}37");
        let text = table(&[running(&label)], &[], None, TableOptions::new(), 0.0);
        assert!(text.contains("me37"), "{hidden:?}: {text}");
        assert!(!text.contains(hidden), "{hidden:?}: {text}");
    }
}

#[test]
fn a_row_that_says_port_zero_has_no_endpoint_to_probe() {
    let row = |port: u64| {
        Instance::from_json(&serde_json::json!(
            {"id": 1, "ssh_host": "ssh4.vast.ai", "ssh_port": port}))
        .unwrap()
    };
    assert_eq!(row(0).ssh_endpoint(), None);
    assert_eq!(row(22).ssh_endpoint(), Some(("ssh4.vast.ai", 22)));
}

#[test]
fn an_integer_written_as_a_float_or_a_float_string_is_still_an_integer() {
    let read = |row: serde_json::Value| Instance::from_json(&row);
    let i = read(
        serde_json::json!({"id": 123.0, "ssh_port": "23484.0", "num_gpus": 2.0,
        "machine_id": "100682.0", "host_id": 5}),
    )
    .unwrap();
    assert_eq!(
        (i.id, i.ssh_port, i.num_gpus, i.machine_id, i.host_id),
        (123, Some(23_484), Some(2), Some(100_682), Some(5))
    );
    // Not an integer: no row, or no value; never a rounded one.
    assert!(read(serde_json::json!({"id": 1.5})).is_none());
    assert!(read(serde_json::json!({"id": -1.0})).is_none());
    assert!(read(serde_json::json!({"id": 1e300})).is_none());
    assert!(read(serde_json::json!({"id": 9_007_199_254_740_993.0_f64})).is_none());
    assert_eq!(
        read(serde_json::json!({"id": "1e3"})).map(|i| i.id),
        Some(1000)
    );
    assert!(read(serde_json::json!({"id": "x"})).is_none());
    let odd =
        read(serde_json::json!({"id": 1, "num_gpus": 2.5, "ssh_port": "NaN", "host_id": null}))
            .unwrap();
    assert_eq!(
        (odd.num_gpus, odd.ssh_port, odd.host_id),
        (None, None, None)
    );
}

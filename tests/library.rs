//! The library as a downstream crate uses it, with no box: readings built by hand with
//! `GpuReading::new` and its setters drive a `Trend` and a table, and the promised helpers are
//! callable from outside the crate.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::unnecessary_wraps
)]
// EXPLICIT: test code -- a failed unwrap or index IS the failed assertion; and `probe_of` returns
// the `ProbeOutcome` (an `Option`) that the API takes.

use vast_skopos::instance::Instance;
use vast_skopos::offer::{country_code, gpu_model};
use vast_skopos::probe::{
    GpuReading, LogReading, Probe, ProbeOutcome, clean, parse_log, ssh_command, strip,
};
use vast_skopos::render::{TableOptions, age, failure_cell, log_cell, short_path, table};
use vast_skopos::trend::Trend;

fn running(id: u64) -> Instance {
    Instance::from_json(&serde_json::json!({"id": id, "actual_status": "running",
        "gpu_name": "RTX 5090", "num_gpus": 1}))
    .unwrap()
}

/// A probe of one GPU, built as a downstream crate can: `Probe::default()` and its fields.
fn probe_of(gpu: GpuReading) -> ProbeOutcome {
    let mut probe = Probe::default();
    probe.gpus = vec![gpu];
    Some(Ok(probe))
}

#[test]
fn readings_built_by_hand_drive_a_trend_and_a_table() {
    let boxes = [running(7)];
    let at_default = GpuReading::new(0, 99, 7_700, 32_607)
        .power(560, 575, 575)
        .clocks(2_900, 3_090)
        .throttle(0x4)
        .processes(1);
    let capped = GpuReading::new(0, 99, 7_700, 32_607)
        .power(399, 400, 575)
        .power_max(575)
        .clocks(1_700, 3_090)
        .throttle(0x4)
        .processes(1);
    let mut trend = Trend::default();
    trend.record(&boxes, &[probe_of(at_default)]);
    assert!(trend.changes().is_empty(), "the first look is the baseline");
    trend.record(&boxes, &[probe_of(capped)]);
    assert_eq!(
        trend
            .changes()
            .iter()
            .map(vast_skopos::trend::GpuChange::line)
            .collect::<Vec<_>>(),
        ["! 7 gpu 0: power limit 575 -> 400 W"]
    );
    let text = table(
        &boxes,
        &[probe_of(capped)],
        None,
        TableOptions::new().ssh(true).trend(&trend),
        0.0,
    );
    assert!(text.contains("400/575W+power_cap"), "{text}");
    assert!(text.contains("2900 1700"), "the clock trend: {text}");
    assert!(
        text.ends_with("! 7 gpu 0: power limit 575 -> 400 W\n"),
        "{text}"
    );
}

#[test]
fn the_promised_helpers_are_callable_from_outside_the_crate() {
    assert_eq!(strip("10%\r100% done"), "100% done");
    assert_eq!(clean(&"x".repeat(200)).chars().count(), 121);
    assert_eq!(
        parse_log("\nvas-grep-file:/r/a.log\nvas-grep-exit:1\n"),
        LogReading::NoMatchingLine {
            file: "/r/a.log".to_owned()
        }
    );
    assert_eq!(age(15_300.0), "4h 15m");
    assert_eq!(short_path("/root/runs/s0/train.log"), "s0/train.log");
    assert_eq!(log_cell(Some(&LogReading::NoMatch)), "(no file matches)");
    let refused = vast_skopos::SkoposError::Ssh {
        target: "h:1".to_owned(),
        message: "Connection refused".to_owned(),
    };
    assert_eq!(
        failure_cell(&refused, Some(240.0)),
        "UNREACHABLE 4m: Connection refused"
    );
    assert_eq!(gpu_model("RTX_5090").as_deref(), Some("RTX 5090"));
    assert_eq!(country_code(" cn ").as_deref(), Some("CN"));
    let command = ssh_command("ssh6.vast.ai", 36_000, None, None);
    assert_eq!(command.get_program(), "ssh");
}

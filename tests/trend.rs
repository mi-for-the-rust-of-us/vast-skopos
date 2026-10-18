//! What a watch keeps of each box's GPUs: the last samples, oldest first, and when it forgets them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unnecessary_wraps
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion; and the helpers return the
// `ProbeOutcome` (an `Option`) that the API takes, whether or not each one could be a bare value.

use std::fmt::Write as _;

use vast_skopos::SkoposError;
use vast_skopos::instance::Instance;
use vast_skopos::probe::{ProbeOutcome, parse_output};
use vast_skopos::trend::{TREND_LEN, Trend};

fn running(id: u64) -> Instance {
    Instance::from_json(&serde_json::json!({"id": id, "actual_status": "running"})).unwrap()
}

/// A probe that read these utilisations, one per GPU.
fn read(utils: &[u32]) -> ProbeOutcome {
    let mut csv = String::new();
    for (index, util) in utils.iter().enumerate() {
        let _ = writeln!(csv, "{index}, {util}, 100, 200");
    }
    Some(Ok(parse_output(&csv)))
}

fn failed() -> ProbeOutcome {
    Some(Err(SkoposError::Ssh {
        target: "h:1".to_owned(),
        message: "Connection refused".to_owned(),
    }))
}

#[test]
fn the_samples_come_oldest_first_and_only_the_last_few_are_kept() {
    let boxes = [running(7)];
    let mut trend = Trend::default();
    assert_eq!(trend.samples(7), Vec::<u32>::new());
    for util in 1..=(u32::try_from(TREND_LEN).unwrap() + 3) {
        trend.record(&boxes, &[read(&[util])]);
    }
    let kept = trend.samples(7);
    assert_eq!(kept.len(), TREND_LEN);
    assert_eq!(kept, [4, 5, 6, 7, 8, 9], "the three oldest are gone");
    assert_eq!(trend.text(7), "4 5 6 7 8 9");
}

#[test]
fn a_box_is_the_mean_of_its_gpus_rounded() {
    let boxes = [running(1), running(2), running(3)];
    let mut trend = Trend::default();
    trend.record(
        &boxes,
        &[read(&[99, 98]), read(&[100, 0]), read(&[0, 0, 1])],
    );
    assert_eq!(trend.samples(1), [99], "98.5 rounds up");
    assert_eq!(trend.samples(2), [50]);
    assert_eq!(trend.samples(3), [0], "0.33 rounds down");
}

#[test]
fn a_probe_that_failed_or_never_ran_adds_nothing_and_keeps_what_the_box_had() {
    let boxes = [running(1), running(2)];
    let mut trend = Trend::default();
    trend.record(&boxes, &[read(&[99]), read(&[98])]);
    // One dropped ssh is not the run stopping: the history stays, and no zero is invented.
    trend.record(&boxes, &[failed(), None]);
    trend.record(&boxes, &[read(&[97])]);
    assert_eq!(trend.samples(1), [99, 97]);
    assert_eq!(
        trend.samples(2),
        [98],
        "a probes slice shorter than the instances reads as not probed"
    );
    // A probe that answered with no GPU line at all is no sample either.
    trend.record(&boxes, &[Some(Ok(parse_output("garbage\n"))), None]);
    assert_eq!(trend.samples(1), [99, 97]);
}

#[test]
fn a_box_that_stopped_or_left_is_forgotten_and_starts_fresh() {
    let mut trend = Trend::default();
    trend.record(&[running(1), running(2)], &[read(&[99]), read(&[99])]);
    // Box 1 stops; box 2 is gone from the list.
    let stopped =
        Instance::from_json(&serde_json::json!({"id": 1, "actual_status": "exited"})).unwrap();
    trend.record(&[stopped], &[None]);
    assert_eq!(
        trend.samples(1),
        Vec::<u32>::new(),
        "its last run says nothing of the next"
    );
    assert_eq!(trend.samples(2), Vec::<u32>::new());
    trend.record(&[running(1)], &[read(&[5])]);
    assert_eq!(trend.samples(1), [5], "fresh, not [99, 5]");
    assert_eq!(trend.text(1), "5");
    assert_eq!(trend.text(2), "-");
}

#[test]
fn boxes_keep_their_own_history() {
    let boxes = [running(1), running(2)];
    let mut trend = Trend::default();
    trend.record(&boxes, &[read(&[10]), read(&[90])]);
    trend.record(&boxes, &[read(&[11]), read(&[91])]);
    assert_eq!(trend.samples(1), [10, 11]);
    assert_eq!(trend.samples(2), [90, 91]);
}

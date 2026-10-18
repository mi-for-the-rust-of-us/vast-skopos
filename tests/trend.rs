//! What a watch keeps of each box's GPUs: the last samples, oldest first, and when it forgets them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unnecessary_wraps,
    clippy::assert_is_empty
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion; and the helpers return the
// `ProbeOutcome` (an `Option`) that the API takes, whether or not each one could be a bare value;
// and `is_empty()` reads better than a comparison with an empty array of changes.

use std::fmt::Write as _;

use vast_skopos::SkoposError;
use vast_skopos::instance::Instance;
use vast_skopos::probe::{ProbeOutcome, parse_output};
use vast_skopos::trend::{PowerChange, TREND_LEN, Trend};

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

/// A probe of one GPU with this power limit (watts, or `[N/A]`) and this throttle mask.
fn powered(limit: &str, mask: &str) -> ProbeOutcome {
    Some(Ok(parse_output(&format!(
        "0, 100, 1, 2\n---vas-power---\n0, 399, {limit}, 575, 1700, 3090, {mask}\n"
    ))))
}

fn lines(trend: &Trend) -> Vec<String> {
    trend.changes().iter().map(PowerChange::line).collect()
}

#[test]
fn the_first_look_is_a_baseline_and_a_limit_that_moves_is_a_change() {
    let boxes = [running(7)];
    let mut trend = Trend::default();
    trend.record(&boxes, &[powered("575", "0x0")]);
    assert!(trend.changes().is_empty());
    trend.record(&boxes, &[powered("575", "0x0")]);
    assert!(trend.changes().is_empty(), "nothing moved");
    trend.record(&boxes, &[powered("400", "0x0")]);
    assert_eq!(lines(&trend), ["! 7 gpu 0: power limit 575 -> 400 W"]);
    assert_eq!(trend.changes()[0].limit, Some((575, 400)));
    // The next look compares with 400, and the change is not repeated.
    trend.record(&boxes, &[powered("400", "0x0")]);
    assert!(trend.changes().is_empty());
}

#[test]
fn a_throttle_reason_that_begins_or_ends_is_a_change_and_so_are_both_at_once() {
    let boxes = [running(7)];
    let mut trend = Trend::default();
    trend.record(&boxes, &[powered("575", "0x0")]);
    trend.record(&boxes, &[powered("575", "0x4")]);
    assert_eq!(lines(&trend), ["! 7 gpu 0: power_cap began"]);
    // The cap ends as the card runs hot; the limit moves with it.
    trend.record(&boxes, &[powered("500", "0x20")]);
    assert_eq!(
        lines(&trend),
        ["! 7 gpu 0: power limit 575 -> 500 W, thermal began, power_cap ended"]
    );
    // An idle card (0x1) is not a restraint: it neither begins nor ends anything.
    trend.record(&boxes, &[powered("500", "0x21")]);
    assert!(trend.changes().is_empty());
    trend.record(&boxes, &[powered("500", "0x0")]);
    assert_eq!(lines(&trend), ["! 7 gpu 0: thermal ended"]);
}

#[test]
fn a_reading_the_box_did_not_give_is_no_change_and_does_not_erase_what_was_known() {
    let boxes = [running(7)];
    let mut trend = Trend::default();
    trend.record(&boxes, &[powered("575", "0x4")]);
    // Neither field this look: the probe stays silent about the power.
    trend.record(&boxes, &[Some(Ok(parse_output("0, 100, 1, 2\n")))]);
    assert!(trend.changes().is_empty());
    // A failed probe adds nothing either.
    trend.record(&boxes, &[failed()]);
    assert!(trend.changes().is_empty());
    // Back with the same limit and cap: still no change, the memory held.
    trend.record(&boxes, &[powered("575", "0x4")]);
    assert!(trend.changes().is_empty());
    trend.record(&boxes, &[powered("[N/A]", "0x4")]);
    assert!(trend.changes().is_empty(), "[N/A] is not a limit of zero");
    trend.record(&boxes, &[powered("400", "0x4")]);
    assert_eq!(lines(&trend), ["! 7 gpu 0: power limit 575 -> 400 W"]);
}

#[test]
fn each_gpu_and_each_box_has_its_own_memory_and_a_stopped_box_forgets() {
    let boxes = [running(1), running(2)];
    let two_gpus = |a: &str, b: &str| -> ProbeOutcome {
        Some(Ok(parse_output(&format!(
            "0, 1, 1, 2\n1, 1, 1, 2\n---vas-power---\n0, 1, {a}, 575, 1, 2, 0x0\n1, 1, {b}, 575, 1, 2, 0x0\n"
        ))))
    };
    let mut trend = Trend::default();
    trend.record(&boxes, &[two_gpus("575", "575"), powered("575", "0x0")]);
    trend.record(&boxes, &[two_gpus("575", "400"), powered("575", "0x4")]);
    assert_eq!(
        lines(&trend),
        [
            "! 1 gpu 1: power limit 575 -> 400 W",
            "! 2 gpu 0: power_cap began"
        ],
        "in the boxes' order, GPU by GPU"
    );
    // Box 2 stops, then runs again with the cap it had: a new run, a new baseline.
    let stopped =
        Instance::from_json(&serde_json::json!({"id": 2, "actual_status": "exited"})).unwrap();
    trend.record(&[running(1), stopped], &[two_gpus("575", "400"), None]);
    trend.record(&boxes, &[two_gpus("575", "400"), powered("400", "0x4")]);
    assert!(trend.changes().is_empty(), "{:?}", lines(&trend));
}

#[test]
fn the_reasons_a_silent_probe_did_not_give_are_still_remembered_when_they_end() {
    let boxes = [running(7)];
    let mut trend = Trend::default();
    trend.record(&boxes, &[powered("575", "0x4")]);
    // The limit comes back but the mask does not: nothing is said, nothing is forgotten.
    trend.record(&boxes, &[powered("575", "[N/A]")]);
    assert!(trend.changes().is_empty());
    trend.record(&boxes, &[powered("575", "0x0")]);
    assert_eq!(lines(&trend), ["! 7 gpu 0: power_cap ended"]);
}

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
use vast_skopos::trend::{GpuChange, TREND_LEN, Trend};

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
    trend.changes().iter().map(GpuChange::line).collect()
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

/// A probe of one GPU at this SM clock (MHz, or `[N/A]`).
fn clocked(mhz: &str) -> ProbeOutcome {
    Some(Ok(parse_output(&format!(
        "0, 100, 1, 2\n---vas-power---\n0, 399, 400, 575, {mhz}, 3090, 0x0\n"
    ))))
}

#[test]
fn the_sm_clock_is_kept_like_the_utilisation_oldest_first_and_trimmed() {
    let boxes = [running(7)];
    let mut trend = Trend::default();
    assert_eq!(trend.clock_text(7), "-");
    for mhz in [3000, 2900, 1700, 1700, 1700, 1700, 1700, 1650] {
        trend.record(&boxes, &[clocked(&mhz.to_string())]);
    }
    assert_eq!(trend.clock_samples(7).len(), TREND_LEN);
    assert_eq!(trend.clock_text(7), "1700 1700 1700 1700 1700 1650");
    // The utilisation went on beside it.
    assert_eq!(trend.samples(7), [100; 6]);
}

#[test]
fn a_clock_sample_is_the_mean_of_the_gpus_that_gave_one_and_a_silent_probe_adds_none() {
    let boxes = [running(7)];
    let mut trend = Trend::default();
    let two = |a: &str, b: &str| -> ProbeOutcome {
        Some(Ok(parse_output(&format!(
            "0, 1, 1, 2\n1, 1, 1, 2\n---vas-power---\n0, 1, 400, 575, {a}, 3090, 0x0\n1, 1, 400, 575, {b}, 3090, 0x0\n"
        ))))
    };
    trend.record(&boxes, &[two("3000", "1701")]);
    assert_eq!(trend.clock_samples(7), [2351], "2350.5 rounds up");
    trend.record(&boxes, &[two("1600", "[N/A]")]);
    assert_eq!(
        trend.clock_samples(7),
        [2351, 1600],
        "the GPU that gave none is left out, not counted as 0 MHz"
    );
    // No clock from any GPU, a probe with no power section, a failed probe: no sample.
    trend.record(&boxes, &[clocked("[N/A]")]);
    trend.record(&boxes, &[Some(Ok(parse_output("0, 100, 1, 2\n")))]);
    trend.record(&boxes, &[failed()]);
    assert_eq!(trend.clock_samples(7), [2351, 1600]);
}

#[test]
fn a_stopped_box_loses_its_clock_history() {
    let mut trend = Trend::default();
    trend.record(&[running(7)], &[clocked("3000")]);
    let stopped =
        Instance::from_json(&serde_json::json!({"id": 7, "actual_status": "exited"})).unwrap();
    trend.record(&[stopped], &[None]);
    assert_eq!(trend.clock_samples(7), Vec::<u32>::new());
    trend.record(&[running(7)], &[clocked("1700")]);
    assert_eq!(trend.clock_text(7), "1700", "fresh, not 3000 1700");
}

#[test]
fn readings_that_would_overflow_a_sum_are_averaged_not_a_panic() {
    // A box is untrusted: two saturated readings add up past `u32::MAX`.
    let boxes = [running(7)];
    let mut trend = Trend::default();
    let huge = format!(
        "0, {m}, 1, 2\n1, {m}, 1, 2\n---vas-power---\n0, 1, 1, 1, {m}, 1, 0x0\n1, 1, 1, 1, {m}, 1, 0x0\n",
        m = u32::MAX
    );
    trend.record(&boxes, &[Some(Ok(parse_output(&huge)))]);
    assert_eq!(trend.samples(7), [u32::MAX]);
    assert_eq!(trend.clock_samples(7), [u32::MAX]);
    // 1e30 MHz saturates at `u32::MAX` as a reading, not at a panic, and the next look goes on.
    let silly = "0, 1, 1, 2\n---vas-power---\n0, 1e30, 1e30, 1e30, 1e30, 1e30, 0x0\n";
    trend.record(&boxes, &[Some(Ok(parse_output(silly)))]);
    assert_eq!(trend.clock_samples(7), [u32::MAX, u32::MAX]);
    assert_eq!(trend.samples(7).len(), 2);
}

#[test]
fn a_gpu_missing_from_a_look_is_remembered_and_compared_when_it_is_back() {
    let boxes = [running(7)];
    let mut trend = Trend::default();
    let two = |a: &str, b: &str| -> ProbeOutcome {
        Some(Ok(parse_output(&format!(
            "0, 1, 1, 2\n1, 1, 1, 2\n---vas-power---\n0, 1, {a}, 575, 1, 2, 0x0\n1, 1, {b}, 575, 1, 2, 0x0\n"
        ))))
    };
    let one = Some(Ok(parse_output(
        "0, 1, 1, 2\n---vas-power---\n0, 1, 575, 575, 1, 2, 0x0\n",
    )));
    trend.record(&boxes, &[two("575", "575")]);
    // GPU 1 is not in this look at all (a dropped line, a GPU that fell off the bus).
    trend.record(&boxes, &[one]);
    assert!(
        trend.changes().is_empty(),
        "a GPU that is absent is not a change"
    );
    // Back with another limit: compared with what it was before it went.
    trend.record(&boxes, &[two("575", "400")]);
    assert_eq!(lines(&trend), ["! 7 gpu 1: power limit 575 -> 400 W"]);
}

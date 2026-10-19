//! The power readings of a probe: the limit a host set, what holds the clocks down, and how the
//! table and the JSON say so. The box in the first test is the one a rental showed on 2026-10-18:
//! an RTX 5090 capped at 400 W against its 575 W default, its SM clock held at 1695 of 3090 MHz.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::unnecessary_wraps
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion; and `outcome` returns the
// `ProbeOutcome` (an `Option`) that the API takes.

use vast_skopos::compact::json_lines;
use vast_skopos::instance::Instance;
use vast_skopos::probe::{ProbeOutcome, parse_output, remote_command};
use vast_skopos::render::{TableOptions, table};

/// What the remote command printed on that box.
const CAPPED: &str = "0, 100, 24745, 32607\n---vas-power---\n0, 399.86, 400.00, 575.00, 1695, 3090, 0x0000000000000004\n";

fn running() -> Instance {
    Instance::from_json(&serde_json::json!({"id": 1, "actual_status": "running"})).unwrap()
}

fn outcome(output: &str) -> ProbeOutcome {
    Some(Ok(parse_output(output)))
}

/// The words of the POWER cell of the box's row.
fn power_cell(output: &str) -> String {
    let text = table(
        &[running()],
        &[outcome(output)],
        None,
        TableOptions::new().ssh(true),
        0.0,
    );
    let header = text.lines().next().unwrap();
    let from = header.find("POWER").unwrap();
    let to = header.find("CLOCK").unwrap();
    text.lines().nth(1).unwrap()[from..to].trim().to_owned()
}

/// The CLOCK cell of the box's row.
fn clock_cell(output: &str) -> String {
    let text = table(
        &[running()],
        &[outcome(output)],
        None,
        TableOptions::new().ssh(true),
        0.0,
    );
    let header = text.lines().next().unwrap();
    let from = header.find("CLOCK").unwrap();
    let to = header.find("LAST LOG").unwrap();
    text.lines().nth(1).unwrap()[from..to].trim().to_owned()
}

#[test]
fn the_clock_cell_says_the_clock_over_the_highest_it_can_reach() {
    assert_eq!(clock_cell(CAPPED), "1695/3090");
    let two = "0, 1, 1, 2\n1, 1, 1, 2\n---vas-power---\n0, 1, 400, 575, 3000, 3090, 0x0\n1, 1, 400, 575, 1700, 3090, 0x0\n";
    assert_eq!(clock_cell(two), "3000/3090 1700/3090");
    // No maximum given: the clock alone. No clock at all: a dash.
    let no_max = "0, 1, 1, 2\n---vas-power---\n0, 1, 400, 575, 1700, [N/A], 0x0\n";
    assert_eq!(clock_cell(no_max), "1700");
    assert_eq!(clock_cell("0, 1, 1, 2\n"), "-");
}

#[test]
fn a_capped_5090_reads_its_limit_its_default_its_clock_and_the_reason() {
    let probe = parse_output(CAPPED);
    let g = &probe.gpus[0];
    assert_eq!(
        (g.util_pct, g.mem_used_mib, g.mem_total_mib),
        (100, 24_745, 32_607)
    );
    assert_eq!(
        (g.power_w, g.power_limit_w, g.power_default_w),
        (Some(400), Some(400), Some(575)),
        "399.86 W is 400 W to the watt"
    );
    assert_eq!((g.sm_mhz, g.sm_max_mhz), (Some(1695), Some(3090)));
    assert_eq!(g.throttle_mask, Some(4));
    assert_eq!(g.throttle_words(), Some(vec!["power_cap"]));
}

#[test]
fn the_table_says_the_limit_against_the_default_and_what_holds_the_card() {
    assert_eq!(power_cell(CAPPED), "400/575W+power_cap");
    // At its default and free: the limit alone.
    let free =
        "0, 100, 1, 2\n---vas-power---\n0, 560.1, 575.00, 575.00, 2900, 3090, 0x0000000000000001\n";
    assert_eq!(
        power_cell(free),
        "575W",
        "an idle reason (0x1) holds nothing down"
    );
    // Capped but not at the default of the card: a limit above the default is shown plainly.
    let above = "0, 1, 1, 2\n---vas-power---\n0, 100, 600, 575, 1, 2, 0x0\n";
    assert_eq!(power_cell(above), "600W");
    // Two reasons at once, and the same word once for the two thermal bits.
    let hot = "0, 1, 1, 2\n---vas-power---\n0, 100, 400, 575, 1, 2, 0x000000000000006c\n";
    assert_eq!(power_cell(hot), "400/575W+power_cap+slowdown+thermal");
    // Two GPUs: one cell each.
    let two = "0, 1, 1, 2\n1, 1, 1, 2\n---vas-power---\n0, 100, 400, 575, 1, 2, 0x4\n1, 100, 575, 575, 1, 2, 0x0\n";
    assert_eq!(power_cell(two), "400/575W+power_cap 575W");
}

#[test]
fn a_box_that_gave_no_power_leaves_every_other_reading_and_a_dash() {
    // An old driver lacks a field of the power query: that section is empty.
    let old = "0, 97, 100, 200\n---vas-power---\n";
    let probe = parse_output(old);
    let g = &probe.gpus[0];
    assert_eq!((g.util_pct, g.mem_total_mib), (97, 200));
    assert_eq!(
        (g.power_w, g.power_limit_w, g.sm_mhz, g.throttle_mask),
        (None, None, None, None)
    );
    assert_eq!(
        g.throttle_words(),
        None,
        "unknown is not 'nothing holds it'"
    );
    assert_eq!(power_cell(old), "-");
    // The section missing altogether, as before this reading existed.
    assert_eq!(power_cell("0, 97, 100, 200\n"), "-");
}

#[test]
fn garbled_power_lines_and_fields_are_skipped_one_by_one() {
    let mixed = "0, 97, 100, 200\n---vas-power---\n\
                 garbage\n\
                 7, 1, 2, 3, 4, 5, 0x4\n\
                 0, [N/A], 400.00, [N/A], -5, nan, not-hex\n";
    let g = &parse_output(mixed).gpus[0];
    assert_eq!(g.power_w, None, "[N/A]");
    assert_eq!(g.power_limit_w, Some(400));
    assert_eq!(g.power_default_w, None);
    assert_eq!(g.sm_mhz, None, "a negative reading");
    assert_eq!(g.sm_max_mhz, None, "nan");
    assert_eq!(g.throttle_mask, None, "text that is not a mask");
    // The GPU 7 the main query never named is not made up.
    assert_eq!(parse_output(mixed).gpus.len(), 1);
}

#[test]
fn the_power_section_and_the_log_section_both_read() {
    let both = format!("{CAPPED}---vas-log---\nvas-file:/r/s0/train.log\nepoch 41\n");
    let probe = parse_output(&both);
    assert_eq!(probe.gpus[0].power_limit_w, Some(400));
    assert!(matches!(
        probe.log,
        Some(vast_skopos::probe::LogReading::Line { .. })
    ));
    // A log line that looks like the power separator is the log's, not a section.
    let forged = "0, 1, 1, 2\n---vas-log---\nvas-file:/r/a.log\n---vas-power---\n";
    let probe = parse_output(forged);
    assert_eq!(probe.gpus[0].power_limit_w, None);
    assert!(probe.log.is_some());
}

#[test]
fn the_remote_command_asks_the_power_apart_so_an_old_driver_costs_only_that() {
    let command = remote_command(Some("/r/*.log"));
    let main = command.find("utilization.gpu").unwrap();
    let power = command.find("power.draw").unwrap();
    let log = command.find("---vas-log---").unwrap();
    assert!(main < power && power < log, "{command}");
    assert!(
        command.contains("clocks_throttle_reasons.active"),
        "the name every driver knows: {command}"
    );
    // Without --log, the power query still runs, and is last, its stderr thrown away: a driver
    // without a field says so on the box's stderr, not on the table.
    let plain = remote_command(None);
    assert!(plain.contains("power.draw") && !plain.contains("---vas-log---"));
    assert!(
        plain.contains(
            "clocks_throttle_reasons.active --format=csv,noheader,nounits 2>/dev/null; echo"
        ),
        "the power query's stderr is thrown away: {plain}"
    );
}

#[test]
fn the_json_carries_the_power_or_nulls_and_the_throttle_as_words() {
    let objects = |output: &str| -> serde_json::Value {
        let text = json_lines(&[running()], &[outcome(output)], None, None, 0.0);
        let first = text.lines().next().unwrap();
        serde_json::from_str::<serde_json::Value>(first).unwrap()["probe"]["gpus"][0].clone()
    };
    let capped = objects(CAPPED);
    assert_eq!(capped["power_w"], 400);
    assert_eq!(capped["power_limit_w"], 400);
    assert_eq!(capped["power_default_w"], 575);
    assert_eq!(capped["sm_mhz"], 1695);
    assert_eq!(capped["sm_max_mhz"], 3090);
    assert_eq!(capped["throttle"], serde_json::json!(["power_cap"]));
    // Free: an empty list. Unknown: null. The keys never change.
    let free = objects("0, 1, 1, 2\n---vas-power---\n0, 100, 575, 575, 1, 2, 0x0\n");
    assert_eq!(free["throttle"], serde_json::json!([]));
    let unknown = objects("0, 1, 1, 2\n");
    assert_eq!(unknown["throttle"], serde_json::Value::Null);
    assert_eq!(
        unknown.as_object().unwrap().keys().collect::<Vec<_>>(),
        capped.as_object().unwrap().keys().collect::<Vec<_>>()
    );
}

/// A trend that has seen the box at its default, then capped.
fn capped_trend() -> vast_skopos::trend::Trend {
    let mut trend = vast_skopos::trend::Trend::default();
    let free = "0, 100, 1, 2\n---vas-power---\n0, 560, 575, 575, 2900, 3090, 0x0\n";
    trend.record(&[running()], &[outcome(free)]);
    trend.record(&[running()], &[outcome(CAPPED)]);
    trend
}

#[test]
fn the_table_of_a_watch_says_what_changed_under_the_money_and_a_one_shot_says_nothing() {
    let trend = capped_trend();
    let text = table(
        &[running()],
        &[outcome(CAPPED)],
        None,
        TableOptions::new().ssh(true).trend(&trend),
        0.0,
    );
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[lines.len() - 2].starts_with("credit "), "{text}");
    assert_eq!(
        lines[lines.len() - 1],
        "! 1 gpu 0: power limit 575 -> 400 W, power_cap began",
        "{text}"
    );
    // No trend (a one-shot), or nothing moved: no such line.
    let once = table(
        &[running()],
        &[outcome(CAPPED)],
        None,
        TableOptions::new().ssh(true),
        0.0,
    );
    assert!(!once.contains("! 1 gpu"), "{once}");
    let mut quiet = capped_trend();
    quiet.record(&[running()], &[outcome(CAPPED)]);
    let again = table(
        &[running()],
        &[outcome(CAPPED)],
        None,
        TableOptions::new().ssh(true).trend(&quiet),
        0.0,
    );
    assert!(!again.contains("! 1 gpu"), "{again}");
}

#[test]
fn compact_prints_the_change_as_an_event_before_the_money_that_closes_the_look() {
    let trend = capped_trend();
    let text = json_lines(&[running()], &[outcome(CAPPED)], Some(&trend), None, 42.0);
    let kinds: Vec<String> = text
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(kinds, ["box", "event", "money"], "{text}");
    let event: serde_json::Value = serde_json::from_str(text.lines().nth(1).unwrap()).unwrap();
    assert_eq!(
        event,
        serde_json::json!({"kind": "event", "at": 42, "id": 1, "gpu": 0,
          "limit_was": 575, "limit_now": 400, "other_mib_was": null, "other_mib_now": null,
          "began": ["power_cap"], "ended": []})
    );
    // A change of reasons only: the limit keys stay, null.
    let mut trend = vast_skopos::trend::Trend::default();
    let free = "0, 1, 1, 2\n---vas-power---\n0, 1, 400, 575, 1, 2, 0x0\n";
    let capped = "0, 1, 1, 2\n---vas-power---\n0, 1, 400, 575, 1, 2, 0x4\n";
    trend.record(&[running()], &[outcome(free)]);
    trend.record(&[running()], &[outcome(capped)]);
    let text = json_lines(&[running()], &[outcome(capped)], Some(&trend), None, 0.0);
    let event: serde_json::Value = serde_json::from_str(text.lines().nth(1).unwrap()).unwrap();
    assert!(event["limit_was"].is_null() && event["limit_now"].is_null());
    // Without a trend there is no event at all.
    let once = json_lines(&[running()], &[outcome(CAPPED)], None, None, 0.0);
    assert_eq!(once.lines().count(), 2);
}

#[test]
fn compact_carries_the_clock_history_of_a_watch_and_null_for_a_one_shot() {
    let mut trend = vast_skopos::trend::Trend::default();
    let at = |mhz: u32| {
        outcome(&format!(
            "0, 100, 1, 2\n---vas-power---\n0, 399, 400, 575, {mhz}, 3090, 0x4\n"
        ))
    };
    for mhz in [3000, 1700] {
        trend.record(&[running()], &[at(mhz)]);
    }
    let probe = |trend: Option<&vast_skopos::trend::Trend>| -> serde_json::Value {
        let text = json_lines(&[running()], &[at(1700)], trend, None, 0.0);
        serde_json::from_str::<serde_json::Value>(text.lines().next().unwrap()).unwrap()["probe"]
            .clone()
    };
    assert_eq!(
        probe(Some(&trend))["clock_trend"],
        serde_json::json!([3000, 1700])
    );
    assert!(probe(None)["clock_trend"].is_null());
}

#[test]
fn a_watch_table_shows_the_clock_history_beside_the_utilisation_history() {
    let mut trend = vast_skopos::trend::Trend::default();
    let at = |mhz: u32| {
        outcome(&format!(
            "0, 100, 1, 2\n---vas-power---\n0, 399, 400, 575, {mhz}, 3090, 0x4\n"
        ))
    };
    for mhz in [3000, 1700] {
        trend.record(&[running()], &[at(mhz)]);
    }
    let text = table(
        &[running()],
        &[at(1700)],
        None,
        TableOptions::new().ssh(true).trend(&trend),
        0.0,
    );
    let row: Vec<&str> = text.lines().nth(1).unwrap().split_whitespace().collect();
    assert!(
        row.windows(4).any(|w| w == ["100", "100", "3000", "1700"]),
        "the utilisation samples, then the clock samples: {text}"
    );
    assert!(row.contains(&"1700/3090"), "{text}");
}

#[test]
fn a_throttle_mask_is_read_with_a_lower_or_upper_case_prefix_and_never_a_doubled_one() {
    let mask = |text: &str| {
        parse_output(&format!(
            "0, 1, 1, 2\n---vas-power---\n0, 1, 400, 575, 1, 2, {text}\n"
        ))
        .gpus[0]
            .throttle_mask
    };
    assert_eq!(mask("0x4"), Some(4));
    assert_eq!(mask("0X4"), Some(4));
    assert_eq!(mask("4"), Some(4));
    assert_eq!(mask("0x0x4"), None, "a doubled prefix is not a mask");
    assert_eq!(mask("0x"), None);
}

#[test]
fn a_throttle_is_shown_even_when_the_limit_is_not_known() {
    // `[N/A]` limit, thermal slowdown: the table must not hide what `--compact` reports.
    let text = "0, 1, 1, 2\n---vas-power---\n0, 1, [N/A], [N/A], 1, 2, 0x20\n";
    assert_eq!(power_cell(text), "-+thermal");
    assert_eq!(
        power_cell("0, 1, 1, 2\n---vas-power---\n0, 1, [N/A], 575, 1, 2, 0x0\n"),
        "-"
    );
}

#[test]
fn the_throttle_reasons_are_their_own_section_and_an_error_message_in_it_is_skipped() {
    // What the box prints: the power and clocks, then the reasons. A driver that lacks the first name
    // for the reasons prints an error line on stdout before the second name answers.
    let text = "0, 100, 1, 2\n---vas-power---\n0, 399.86, 400, 575, 1695, 3090\n\
                ---vas-throttle---\nField \"clocks_event_reasons.active\" is not a valid field to query.\n\
                0, 0x0000000000000004\n";
    let g = &parse_output(text).gpus[0];
    assert_eq!(g.throttle_words(), Some(vec!["power_cap"]));
    assert_eq!((g.power_limit_w, g.sm_mhz), (Some(400), Some(1695)));
    // A driver with neither name: the power and clocks are still read.
    let none =
        "0, 100, 1, 2\n---vas-power---\n0, 399.86, 400, 575, 1695, 3090\n---vas-throttle---\n";
    let g = &parse_output(none).gpus[0];
    assert_eq!((g.power_limit_w, g.sm_max_mhz), (Some(400), Some(3090)));
    assert_eq!(g.throttle_mask, None);
    // The reasons of a GPU the main query did not name are not invented; a bad mask is none.
    let odd = "0, 100, 1, 2\n---vas-power---\n---vas-throttle---\n7, 0x4\n0, 0x0x4\n";
    assert_eq!(parse_output(odd).gpus[0].throttle_mask, None);
    // The section before the apps one, and the apps one still reads.
    let both = "0, 100, 24809, 32607\n---vas-power---\n---vas-throttle---\n0, 0x20\n\
                ---vas-apps---\n0, b\n---vas-apps-by-gpu---\nb, 8720\nvas-apps-ok\n";
    let g = &parse_output(both).gpus[0];
    assert_eq!(g.throttle_words(), Some(vec!["thermal"]));
    assert_eq!(g.other_mem_mib, Some(16_089));
}

#[test]
fn the_reasons_are_asked_apart_from_the_power_under_both_names_of_the_field() {
    let command = remote_command(None);
    let power = command.find("clocks.max.sm --format").unwrap();
    let separator = command.find("---vas-throttle---").unwrap();
    let new = command.find("clocks_event_reasons.active").unwrap();
    let old = command.find("clocks_throttle_reasons.active").unwrap();
    let apps = command.find("---vas-apps---").unwrap();
    assert!(
        power < separator && separator < new && new < old && old < apps,
        "{command}"
    );
    // The power query no longer carries the reasons: a driver without them keeps its power.
    assert!(!command[..separator].contains("reasons"), "{command}");
    // The old name is the fallback of the new one (`||`), and each call is bounded on the box.
    assert!(command[new..old].contains("||"), "{command}");
    assert_eq!(
        command.matches("nvidia-smi").count(),
        command.matches("timeout 20 nvidia-smi").count(),
        "{command}"
    );
}

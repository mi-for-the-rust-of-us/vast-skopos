//! Memory held by processes outside the container: the device's memory in use minus what the
//! processes the box lists hold. The first fixture is the slowed box of 2026-10-18 (an RTX 5090
//! holding 24,809 MiB while its only visible process held 8,720), the second its twin that added up.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::unnecessary_wraps,
    clippy::assert_is_empty
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion; `outcome` returns the
// `ProbeOutcome` (an `Option`) that the API takes; and `is_empty()` reads better than a comparison
// with an empty array of changes.

use vast_skopos::compact::json_lines;
use vast_skopos::instance::Instance;
use vast_skopos::probe::{LogQuery, OTHER_MIB_MIN, ProbeOutcome, parse_output, remote_command};
use vast_skopos::render::{TableOptions, table};
use vast_skopos::trend::{GpuChange, Trend};

/// One GPU holding `used` MiB, then the apps section with these bus ids and processes.
fn output(used: u64, buses: &str, apps: &str, ok: bool) -> String {
    let ok = if ok { "vas-apps-ok\n" } else { "" };
    format!(
        "0, 100, {used}, 32607\n---vas-power---\n---vas-apps---\n{buses}---vas-apps-by-gpu---\n{apps}{ok}"
    )
}

/// The slowed box: 24,809 MiB in use, one process of 8,720.
fn slowed() -> String {
    output(
        24_809,
        "0, 00000000:C1:00.0\n",
        "00000000:C1:00.0, 8720\n",
        true,
    )
}

fn running() -> Instance {
    Instance::from_json(&serde_json::json!({"id": 1, "actual_status": "running"})).unwrap()
}

fn outcome(text: &str) -> ProbeOutcome {
    Some(Ok(parse_output(text)))
}

fn other(text: &str) -> Option<u64> {
    parse_output(text).gpus[0].other_mem_mib
}

#[test]
fn the_slowed_box_holds_16_gib_that_no_process_of_its_own_accounts_for() {
    let probe = parse_output(&slowed());
    let g = &probe.gpus[0];
    assert_eq!(g.mem_used_mib, 24_809);
    assert_eq!(g.other_mem_mib, Some(24_809 - 8_720));
    assert_eq!(g.other_user_mib(), Some(16_089));
    // Its twin adds up: 8,782 in use, 8,772 held by its process: the driver's own 10 MiB.
    let twin = output(
        8_782,
        "0, 00000000:C1:00.0\n",
        "00000000:C1:00.0, 8772\n",
        true,
    );
    let g = &parse_output(&twin).gpus[0];
    assert_eq!(g.other_mem_mib, Some(10));
    assert_eq!(
        g.other_user_mib(),
        None,
        "10 MiB is the driver's, not someone else's"
    );
}

#[test]
fn the_threshold_is_a_gibibyte_and_the_gap_is_never_negative() {
    let at = |used: u64| {
        let g = parse_output(&output(
            used,
            "0, 00000000:C1:00.0\n",
            "00000000:C1:00.0, 1000\n",
            true,
        ))
        .gpus[0];
        g.other_user_mib()
    };
    assert_eq!(OTHER_MIB_MIN, 1024);
    assert_eq!(at(1000 + OTHER_MIB_MIN - 1), None);
    assert_eq!(at(1000 + OTHER_MIB_MIN), Some(OTHER_MIB_MIN));
    // A process that holds more than the device says it does (the two are read a moment apart).
    assert_eq!(other(&output(500, "0, b\n", "b, 1000\n", true)), Some(0));
}

#[test]
fn the_processes_of_a_gpu_are_summed_and_the_other_gpus_are_not_counted() {
    let text = "0, 100, 30000, 32607\n1, 100, 20000, 32607\n---vas-power---\n---vas-apps---\n\
                0, 00000000:C1:00.0\n1, 00000000:C2:00.0\n---vas-apps-by-gpu---\n\
                00000000:C1:00.0, 8000\n00000000:C1:00.0, 2000\n00000000:C2:00.0, 19500\nvas-apps-ok\n";
    let gpus = parse_output(text).gpus;
    assert_eq!(gpus[0].other_mem_mib, Some(20_000), "30000 - (8000 + 2000)");
    assert_eq!(gpus[1].other_mem_mib, Some(500));
    // The bus ids are compared whatever their case.
    let upper = "0, 100, 5000, 32607\n---vas-power---\n---vas-apps---\n0, 00000000:c1:00.0\n\
                 ---vas-apps-by-gpu---\n00000000:C1:00.0, 1000\nvas-apps-ok\n";
    assert_eq!(parse_output(upper).gpus[0].other_mem_mib, Some(4000));
}

#[test]
fn a_gpu_with_no_process_listed_is_unknown_not_everyone_elses() {
    // The query succeeded and listed nothing for this GPU: a run that has just restarted, or a box
    // whose container cannot see its own processes, looks exactly like that while the memory is
    // still its own. Claiming 16 GiB of "someone else" there would be a false alarm.
    let text = output(16_000, "0, 00000000:C1:00.0\n", "", true);
    assert_eq!(other(&text), None);
    // Another GPU's process does not make this one known.
    let two = "0, 1, 16000, 2\n1, 1, 100, 2\n---vas-power---\n---vas-apps---\n0, a\n1, b\n\
               ---vas-apps-by-gpu---\nb, 100\nvas-apps-ok\n";
    let gpus = parse_output(two).gpus;
    assert_eq!(gpus[0].other_mem_mib, None);
    assert_eq!(gpus[1].other_mem_mib, Some(0));
}

#[test]
fn nothing_is_concluded_when_the_box_cannot_say() {
    // The query did not end in its success line: an empty list could be a failed query.
    assert_eq!(other(&output(24_809, "0, b\n", "", false)), None);
    // A figure that is not a number (`[N/A]` under some drivers) is not zero.
    assert_eq!(
        other(&output(24_809, "0, b\n", "b, [N/A]\nb, 100\n", true)),
        None
    );
    // The GPU's bus id is unknown.
    assert_eq!(other(&output(24_809, "", "b, 8720\n", true)), None);
    assert_eq!(other(&output(24_809, "1, b\n", "b, 8720\n", true)), None);
    // The section is missing altogether (an old driver, a box that is not NVIDIA's).
    assert_eq!(other("0, 100, 24809, 32607\n"), None);
    assert_eq!(
        other("0, 100, 24809, 32607\n---vas-power---\n0, 1, 400, 575, 1, 2, 0x0\n"),
        None
    );
    // A figure of another GPU that does not parse does not spoil this one.
    let two = "0, 1, 9000, 2\n1, 1, 9000, 2\n---vas-power---\n---vas-apps---\n0, a\n1, b\n\
               ---vas-apps-by-gpu---\na, 1000\nb, [N/A]\nvas-apps-ok\n";
    let gpus = parse_output(two).gpus;
    assert_eq!(gpus[0].other_mem_mib, Some(8000));
    assert_eq!(gpus[1].other_mem_mib, None);
}

#[test]
fn the_log_section_can_follow_the_apps_section() {
    let text = format!("{}---vas-log---\nvas-file:/r/a.log\nepoch 1\n", slowed());
    let probe = parse_output(&text);
    assert_eq!(probe.gpus[0].other_user_mib(), Some(16_089));
    assert!(probe.log.is_some());
}

#[test]
fn the_remote_command_asks_the_processes_last_and_apart() {
    let command = remote_command(Some(&LogQuery::new("/r/*.log")));
    let power = command.find("power.draw").unwrap();
    let apps = command.find("---vas-apps---").unwrap();
    let by_gpu = command.find("---vas-apps-by-gpu---").unwrap();
    let log = command.find("---vas-log---").unwrap();
    assert!(power < apps && apps < by_gpu && by_gpu < log, "{command}");
    assert!(
        command.contains("--query-gpu=index,pci.bus_id"),
        "{command}"
    );
    assert!(
        command.contains("--query-compute-apps=gpu_bus_id,used_gpu_memory"),
        "{command}"
    );
    // The success line follows the processes' query only if it succeeded (`&&`).
    assert!(
        command.contains("nounits 2>/dev/null && echo 'vas-apps-ok'"),
        "{command}"
    );
    // Every nvidia-smi call is bounded on the box.
    assert_eq!(
        command.matches("nvidia-smi").count(),
        command.matches("timeout 20 nvidia-smi").count(),
        "{command}"
    );
}

#[test]
fn the_table_has_an_other_column_that_says_the_memory_or_a_dash() {
    let cell = |text: &str| -> String {
        let table = table(
            &[running()],
            &[outcome(text)],
            None,
            TableOptions::new().ssh(true),
            0.0,
        );
        let header = table.lines().next().unwrap();
        let from = header.find("OTHER").unwrap();
        let to = header.find("POWER").unwrap();
        table.lines().nth(1).unwrap()[from..to].trim().to_owned()
    };
    assert_eq!(cell(&slowed()), "15.7G");
    let twin = output(8_782, "0, b\n", "b, 8772\n", true);
    assert_eq!(cell(&twin), "-");
    assert_eq!(cell("0, 100, 1, 2\n"), "-", "unknown");
    let two = "0, 1, 30000, 2\n1, 1, 5000, 2\n---vas-power---\n---vas-apps---\n0, a\n1, b\n\
               ---vas-apps-by-gpu---\na, 8000\nb, 5000\nvas-apps-ok\n";
    assert_eq!(cell(two), "21.5G -");
}

#[test]
fn compact_carries_the_raw_gap_and_null_when_the_box_did_not_say() {
    let gpu = |text: &str| -> serde_json::Value {
        let out = json_lines(&[running()], &[outcome(text)], None, None, 0.0);
        serde_json::from_str::<serde_json::Value>(out.lines().next().unwrap()).unwrap()["probe"]
            ["gpus"][0]
            .clone()
    };
    assert_eq!(gpu(&slowed())["other_mem_mib"], 16_089);
    let twin = output(8_782, "0, b\n", "b, 8772\n", true);
    assert_eq!(
        gpu(&twin)["other_mem_mib"],
        10,
        "the raw gap, not the threshold"
    );
    assert!(gpu("0, 100, 1, 2\n")["other_mem_mib"].is_null());
}

fn lines(trend: &Trend) -> Vec<String> {
    trend.changes().iter().map(GpuChange::line).collect()
}

#[test]
fn a_watch_says_when_someone_else_begins_or_stops_using_the_gpu() {
    let mut trend = Trend::default();
    let twin = output(8_782, "0, b\n", "b, 8772\n", true);
    trend.record(&[running()], &[outcome(&twin)]);
    assert!(trend.changes().is_empty(), "the first look is the baseline");
    // Someone else takes 16 GiB.
    trend.record(&[running()], &[outcome(&slowed())]);
    assert_eq!(
        lines(&trend),
        ["! 1 gpu 0: memory held outside the container 0.0 -> 15.7 GiB"]
    );
    let change = &trend.changes()[0];
    assert_eq!(change.other_mib, Some((10, 16_089)));
    // Still there: said once. Moving within the notable range is not news.
    trend.record(&[running()], &[outcome(&slowed())]);
    assert!(trend.changes().is_empty());
    let less = output(20_000, "0, b\n", "b, 8720\n", true);
    trend.record(&[running()], &[outcome(&less)]);
    assert!(
        trend.changes().is_empty(),
        "11 GiB instead of 15.7: still someone"
    );
    // They leave.
    trend.record(&[running()], &[outcome(&twin)]);
    assert_eq!(
        lines(&trend),
        ["! 1 gpu 0: memory held outside the container 11.0 -> 0.0 GiB"]
    );
}

#[test]
fn a_look_that_could_not_say_neither_changes_nor_forgets() {
    let mut trend = Trend::default();
    let twin = output(8_782, "0, b\n", "b, 8772\n", true);
    trend.record(&[running()], &[outcome(&slowed())]);
    // The processes' list is missing for a look: nothing is said, and nothing is forgotten.
    trend.record(&[running()], &[outcome("0, 100, 24809, 32607\n")]);
    assert!(trend.changes().is_empty());
    trend.record(&[running()], &[outcome(&twin)]);
    assert_eq!(lines(&trend).len(), 1, "they left: {:?}", lines(&trend));
}

#[test]
fn compact_prints_the_arrival_as_an_event_and_the_table_prints_the_line() {
    let mut trend = Trend::default();
    let twin = output(8_782, "0, b\n", "b, 8772\n", true);
    trend.record(&[running()], &[outcome(&twin)]);
    trend.record(&[running()], &[outcome(&slowed())]);
    let out = json_lines(&[running()], &[outcome(&slowed())], Some(&trend), None, 7.0);
    let event: serde_json::Value = serde_json::from_str(out.lines().nth(1).unwrap()).unwrap();
    assert_eq!(event["kind"], "event");
    assert_eq!(event["other_mib_was"], 10);
    assert_eq!(event["other_mib_now"], 16_089);
    assert!(event["limit_was"].is_null() && event["began"] == serde_json::json!([]));
    let text = table(
        &[running()],
        &[outcome(&slowed())],
        None,
        TableOptions::new().ssh(true).trend(&trend),
        0.0,
    );
    assert!(
        text.lines()
            .last()
            .unwrap()
            .starts_with("! 1 gpu 0: memory held outside the container"),
        "{text}"
    );
}

#[test]
fn a_run_that_restarts_is_not_someone_else_arriving() {
    let mut trend = Trend::default();
    let ours = output(8_782, "0, b\n", "b, 8772\n", true);
    // The run restarts: the card still holds its memory for a look, with no process listed.
    let restarting = output(8_782, "0, b\n", "", true);
    for text in [&ours, &restarting, &ours, &restarting, &restarting, &ours] {
        trend.record(&[running()], &[outcome(text)]);
        assert!(trend.changes().is_empty(), "{:?}", lines(&trend));
    }
}

#[test]
fn a_gap_that_hovers_around_the_threshold_is_news_each_time_it_crosses_it() {
    // There is no hysteresis, on purpose and said so (README): 1 GiB is the line, and a box whose
    // other user hovers around it says so at every crossing, which is the truth of what it sees.
    let mut trend = Trend::default();
    let at = |gap: u64| output(8_772 + gap, "0, b\n", "b, 8772\n", true);
    trend.record(&[running()], &[outcome(&at(0))]);
    let mut said = 0;
    for gap in [2_000, 100, 2_000, 100] {
        trend.record(&[running()], &[outcome(&at(gap))]);
        said += trend.changes().len();
    }
    assert_eq!(said, 4, "up, down, up, down");
    // Moving within a side of the line is not news.
    trend.record(&[running()], &[outcome(&at(2_000))]);
    trend.record(&[running()], &[outcome(&at(3_000))]);
    assert_eq!(trend.changes().len(), 0);
}

const BUS: &str = "0, 00000000:C1:00.0\n";

#[test]
fn a_busy_reading_with_no_memory_and_no_process_is_an_idle_counter() {
    // 2026-10-26: a box nobody had used yet read 100 %, 2 MiB, and no compute process.
    let idle = output(2, BUS, "", true);
    let probe = parse_output(&idle);
    assert_eq!(probe.gpus[0].processes, Some(0));
    assert!(probe.gpus[0].idle_counter());
    let text = table(
        &[running()],
        &[outcome(&idle)],
        None,
        TableOptions::new().ssh(true),
        0.0,
    );
    let row = text.lines().nth(1).unwrap();
    assert!(row.contains("100%?"), "{text}");
    let lines: Vec<&str> = text.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.contains("an idle counter?"))
        .unwrap();
    assert_eq!(
        lines[at],
        "1 gpu 0: 100% with no memory held and no process (an idle counter?)"
    );
    assert!(
        lines[at + 1].starts_with("credit "),
        "before the money: {text}"
    );
    let json = json_lines(&[running()], &[outcome(&idle)], None, None, 0.0);
    let first: serde_json::Value = serde_json::from_str(json.lines().next().unwrap()).unwrap();
    assert_eq!(first["probe"]["gpus"][0]["processes"], 0, "{first}");
}

#[test]
fn a_card_at_work_or_one_whose_processes_are_unknown_is_not_an_idle_counter() {
    let not_idle = |text: &str, why: &str| {
        let probe = parse_output(text);
        assert!(!probe.gpus[0].idle_counter(), "{why}: {probe:?}");
        let table = table(
            &[running()],
            &[outcome(text)],
            None,
            TableOptions::new().ssh(true),
            0.0,
        );
        assert!(
            !table.contains("100%?") && !table.contains("idle counter"),
            "{why}: {table}"
        );
    };
    // The training at work: one process, 7.7 GB.
    let busy = output(7_700, BUS, "00000000:C1:00.0, 7600\n", true);
    assert_eq!(parse_output(&busy).gpus[0].processes, Some(1));
    not_idle(&busy, "a process at work");
    // No process listed, but the memory is held: someone outside the container (`OTHER`).
    not_idle(&output(16_000, BUS, "", true), "memory held");
    // The process query did not end with its success line: unknown, never "no process".
    let unknown = output(2, BUS, "", false);
    assert_eq!(parse_output(&unknown).gpus[0].processes, None);
    not_idle(&unknown, "the process list is unknown");
    // A GPU whose bus id is not given cannot be matched to its processes.
    let no_bus = output(2, "", "", true);
    assert_eq!(parse_output(&no_bus).gpus[0].processes, None);
    not_idle(&no_bus, "no bus id");
    // Two processes are counted.
    let two = output(
        9_000,
        BUS,
        "00000000:C1:00.0, 4000\n00000000:c1:00.0, 4000\n",
        true,
    );
    assert_eq!(parse_output(&two).gpus[0].processes, Some(2));
}

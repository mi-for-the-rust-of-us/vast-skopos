//! The table, from fixtures.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

mod common;

use vast_skopos::SkoposError;
use vast_skopos::instance::{Account, Instance};
use vast_skopos::offer::{OfferPage, OfferQuery, OfferSort, select};
use vast_skopos::probe::parse_output;
use vast_skopos::render::{TableOptions, money_line, offers_footer, offers_table, table};

fn instance(json: &serde_json::Value) -> Instance {
    Instance::from_json(json).unwrap()
}

#[test]
fn the_table_shows_cost_status_probe_and_money() {
    let now = 10_000.0;
    let rows = vec![
        instance(
            &serde_json::json!({"id": 56_053_340, "actual_status": "running", "gpu_name": "RTX 5090",
            "num_gpus": 1, "dph_total": 0.643, "start_date": now - 7200.0,
            "ssh_host": "ssh4.vast.ai", "ssh_port": 23484, "geolocation": "US"}),
        ),
        instance(
            &serde_json::json!({"id": 56_053_638, "actual_status": "exited", "dph_total": 0.608,
            "start_date": now - 600.0}),
        ),
    ];
    let probes = vec![
        Some(Ok(parse_output(
            "0, 98, 30122, 32607\n---vas-log---\nvas-file:/root/runs/s0/train.log\n[ 796.4s] layer 4: ...\n",
        ))),
        Some(Err(SkoposError::Ssh {
            target: "h:1".to_owned(),
            message: "Connection refused".to_owned(),
        })),
    ];
    let account = Account::from_json(&serde_json::json!({"credit": 5.94}));
    let text = table(
        &rows,
        &probes,
        Some(&account),
        TableOptions::new().ssh(true),
        now,
    );
    assert!(text.contains("1x RTX 5090"), "{text}");
    assert!(
        text.contains("2h 0m") && text.contains("1.29"),
        "age and cost: {text}"
    );
    assert!(
        text.contains("98%") && text.contains("29.4/32G"),
        "probe: {text}"
    );
    assert!(
        text.contains("s0/train.log: [ 796.4s] layer 4"),
        "the log cell names the run: {text}"
    );
    assert!(text.contains("Connection refused"), "ssh error: {text}");
    assert!(
        text.contains("credit $5.94 | burn $0.643/h over 1 running | ~9.2 h left"),
        "{text}"
    );
}

#[test]
fn no_instances_still_prints_the_money_line() {
    let text = table(&[], &[], None, TableOptions::new(), 0.0);
    assert!(
        text.contains("(no instances)") && text.contains("credit ? | burn $0.000/h over 0 running")
    );
}

#[test]
fn ssh_columns_show_when_asked_even_with_nothing_probed() {
    let rows = vec![instance(
        &serde_json::json!({"id": 1, "actual_status": "exited"}),
    )];
    let text = table(&rows, &[None], None, TableOptions::new().ssh(true), 0.0);
    assert!(text.contains("GPU% (ssh)"), "{text}");
}

#[test]
fn wide_adds_the_machine_and_the_host_after_ssh_and_before_the_probe_columns() {
    let rows = vec![
        instance(
            &serde_json::json!({"id": 56_362_434, "actual_status": "running",
            "ssh_host": "ssh6.vast.ai", "ssh_port": 36_000, "machine_id": 100_682, "host_id": 406_325}),
        ),
        instance(&serde_json::json!({"id": 2, "actual_status": "exited"})),
    ];
    let narrow = table(
        &rows,
        &[None, None],
        None,
        TableOptions::new().ssh(true),
        0.0,
    );
    assert!(
        !narrow.contains("MACHINE") && !narrow.contains("100682"),
        "{narrow}"
    );
    let text = table(
        &rows,
        &[None, None],
        None,
        TableOptions::new().ssh(true).wide(true),
        0.0,
    );
    let lines: Vec<&str> = text.lines().collect();
    let header: Vec<&str> = lines[0].split_whitespace().collect();
    assert_eq!(&header[8..12], ["SSH", "MACHINE", "HOST", "GPU%"], "{text}");
    let first: Vec<&str> = lines[1].split_whitespace().collect();
    assert!(
        first
            .windows(3)
            .any(|w| w == ["ssh6.vast.ai:36000", "100682", "406325"]),
        "the ids sit under their headers: {text}"
    );
    let second: Vec<&str> = lines[2].split_whitespace().collect();
    assert!(
        second.windows(2).any(|w| w == ["-", "-"]),
        "a box the API gave no ids for shows dashes: {text}"
    );
}

#[test]
fn the_log_cell_tells_no_file_from_an_empty_file() {
    let rows = vec![
        instance(&serde_json::json!({"id": 1, "actual_status": "running"})),
        instance(&serde_json::json!({"id": 2, "actual_status": "running"})),
    ];
    let probes = vec![
        Some(Ok(parse_output(
            "0, 0, 0, 32607\n---vas-log---\nvas-no-file\n",
        ))),
        Some(Ok(parse_output(
            "0, 0, 0, 32607\n---vas-log---\nvas-file:/root/runs/s1/train.log\n",
        ))),
    ];
    let text = table(&rows, &probes, None, TableOptions::new().ssh(true), 0.0);
    assert!(text.contains("(no file matches)"), "{text}");
    assert!(text.contains("s1/train.log: (empty)"), "{text}");
}

#[test]
fn the_offers_table_of_real_rows_is_aligned_and_footed() {
    let answer: serde_json::Value = serde_json::from_str(&common::fixture("offers.json")).unwrap();
    let page = OfferPage::from_json(&answer).unwrap();
    let mut q = OfferQuery::default();
    q.verified = true;
    // BORROW: explicit clone -- the page is read again by the footer
    let kept = select(page.offers.clone(), &q, OfferSort::Price);
    let text = offers_table(&kept, 2);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "the header and the two rows asked for: {text}"
    );
    // Each cell starts where its header does (the cells before `$/H` are ASCII).
    let col = lines[0].find("$/H").unwrap();
    assert_eq!(&lines[1][col..col + 5], "0.540", "{text}");
    assert_eq!(&lines[2][col..col + 5], "0.647", "{text}");
    assert!(
        lines[1].starts_with("44614424") && lines[1].ends_with("United States, US"),
        "{text}"
    );
    assert_eq!(
        offers_footer(&page, &kept, 2, &q, OfferSort::Price, 56_527.0),
        "2 of 3 offers shown, cheapest first | verified | $/h includes 5 GB of storage \
         | searched 15:42:07 UTC\n2 dropped here: a field contradicts a constraint\n"
    );
}

#[test]
fn an_answer_at_the_cap_says_the_list_may_be_cut() {
    let rows: Vec<serde_json::Value> = (0..64).map(|id| serde_json::json!({"id": id})).collect();
    let page =
        OfferPage::from_json(&serde_json::json!({"offers": rows, "truncated": false})).unwrap();
    let footer = offers_footer(
        &page,
        &page.offers,
        20,
        &OfferQuery::default(),
        OfferSort::Price,
        0.0,
    );
    assert!(
        footer.contains("64 returned, vast.ai's cap: the list may be cut"),
        "{footer}"
    );
}

#[test]
fn the_server_saying_truncated_and_rows_without_an_id_are_noted() {
    let page = OfferPage::from_json(&serde_json::json!(
        {"offers": [{"id": 1}, {"gpu_name": "no id"}], "truncated": true}))
    .unwrap();
    let footer = offers_footer(
        &page,
        &page.offers,
        1,
        &OfferQuery::default(),
        OfferSort::Price,
        0.0,
    );
    assert!(
        footer.ends_with(
            "vast.ai says the list was cut (narrow the search) | 1 without an id skipped\n"
        ),
        "{footer}"
    );
}

#[test]
fn a_long_host_written_cell_is_capped_and_its_hidden_characters_dropped() {
    let long = format!("RTX\u{202e} {}", "9".repeat(500));
    let offer = vast_skopos::offer::Offer::from_json(&serde_json::json!(
        {"id": 1, "gpu_name": long, "geolocation": format!("{}, US", "Z".repeat(500))}))
    .unwrap();
    let text = offers_table(&[offer], 1);
    let row = text.lines().nth(1).unwrap();
    assert!(!row.contains('\u{202e}'), "{row}");
    assert!(
        row.chars().count() < 200,
        "two capped cells, not 1000 characters: {row}"
    );
    assert_eq!(row.matches('…').count(), 2, "{row}");
}

/// One instance row for the money line: its actual and intended status, and its `$/h`.
fn fleet_row(id: u64, actual: Option<&str>, intended: Option<&str>, dph: Option<f64>) -> Instance {
    let mut row = serde_json::json!({"id": id});
    if let Some(a) = actual {
        row["actual_status"] = a.into();
    }
    if let Some(i) = intended {
        row["intended_status"] = i.into();
    }
    if let Some(d) = dph {
        row["dph_total"] = d.into();
    }
    instance(&row)
}

#[test]
fn the_burn_counts_a_box_meant_to_run_that_reads_otherwise_and_names_it() {
    let credit = Account::from_json(&serde_json::json!({"credit": 12.0}));
    let fleet = [
        fleet_row(1, Some("running"), Some("running"), Some(0.5)),
        fleet_row(2, Some("offline"), Some("running"), Some(0.25)),
        fleet_row(3, Some("loading"), Some("running"), Some(0.25)),
        fleet_row(4, Some("offline"), Some("running"), Some(0.25)),
        fleet_row(5, Some("exited"), Some("stopped"), Some(9.0)),
    ];
    assert_eq!(
        money_line(&fleet, Some(&credit)),
        "credit $12.00 | burn $1.250/h over 1 running + 1 loading, 2 offline \
         (meant to run, billing unknown) | ~9.6 h left at this rate\n",
        "the runway uses the burn with the uncertain boxes in it; a stopped box is not counted"
    );
}

#[test]
fn a_box_being_stopped_is_still_running_and_a_stopped_one_is_not_named() {
    let fleet = [
        fleet_row(1, Some("running"), Some("stopped"), Some(0.5)),
        fleet_row(2, Some("exited"), Some("stopped"), Some(0.5)),
        fleet_row(3, Some("exited"), None, Some(0.5)),
    ];
    assert_eq!(
        money_line(&fleet, None),
        "credit ? | burn $0.500/h over 1 running\n"
    );
}

#[test]
fn a_box_meant_to_run_with_no_status_or_no_price_is_named_not_hidden() {
    let fleet = [
        fleet_row(1, None, Some("running"), None),
        fleet_row(2, Some("running"), Some("running"), Some(0.5)),
    ];
    assert_eq!(
        money_line(&fleet, None),
        "credit ? | burn $0.500/h over 1 running + 1 unknown (meant to run, billing unknown)\n"
    );
}

/// An answer with no rows, that the server said was cut (or not): all the notes read of a page.
fn page(cut: bool) -> OfferPage {
    OfferPage::from_json(&serde_json::json!({"offers": [], "truncated": cut})).unwrap()
}

/// One offer for the watch tests: id, price, in Spain.
fn watched(id: u64, price: f64) -> vast_skopos::offer::Offer {
    vast_skopos::offer::Offer::from_json(&serde_json::json!(
        {"id": id, "dph_total": price, "gpu_name": "RTX 5090", "num_gpus": 1,
         "geolocation": "Spain, ES"}))
    .unwrap()
}

/// A watch that learned the market as `before` and then looked at `now`: it and what it reports.
fn moved(
    before: &[vast_skopos::offer::Offer],
    now: &[vast_skopos::offer::Offer],
) -> (
    vast_skopos::market::MarketWatch,
    vast_skopos::offer::OfferChanges,
) {
    let mut watch = vast_skopos::market::MarketWatch::default();
    for _ in 0..vast_skopos::market::LEARNING_LOOKS {
        assert!(watch.record(before).is_none());
    }
    let changes = watch.record(now).unwrap();
    (watch, changes)
}

/// `watched`, on the machine `machine`: its offer id changes, it does not.
fn on_machine(id: u64, machine: u64, price: f64) -> vast_skopos::offer::Offer {
    vast_skopos::offer::Offer::from_json(&serde_json::json!(
        {"id": id, "machine_id": machine, "num_gpus": 1, "dph_total": price,
         "gpu_name": "RTX 5090", "geolocation": "Spain, ES"}))
    .unwrap()
}

#[test]
fn the_marked_table_keeps_its_columns_aligned_with_a_mark_and_a_was() {
    use vast_skopos::market::MarketWatch;
    use vast_skopos::offer::OfferChanges;
    use vast_skopos::render::offers_table_marked;
    let before = [watched(1, 0.5), watched(2, 0.6), watched(7, 0.9)];
    let now = [watched(3, 0.4), watched(1, 0.55), watched(2, 0.6)];
    let (watch, changes) = moved(&before, &now);
    let text = offers_table_marked(&now, 20, &changes, &watch);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "the header and three rows: {text}");
    // The mark column is the first, one character wide; every id starts in the same column.
    let id_col = lines[0].find("ID").unwrap();
    assert!(lines[1].starts_with("+ "), "{text}");
    for line in &lines[2..] {
        assert!(line.starts_with("  "), "no mark: {text}");
    }
    for (line, id) in lines[1..].iter().zip(["3", "1", "2"]) {
        assert_eq!(&line[id_col..=id_col], id, "{text}");
    }
    assert!(lines[2].contains("0.550 (was 0.500)"), "{text}");
    assert!(!lines[3].contains("was"), "{text}");
    // SEEN sits just before WHERE and says in how many looks each machine appeared.
    assert!(
        lines[0].find("SEEN").unwrap() < lines[0].find("WHERE").unwrap(),
        "{text}"
    );
    let seen = |line: &str| line.split_whitespace().rev().nth(2).unwrap().to_owned();
    assert_eq!(
        seen(lines[1]),
        "1/1",
        "the new machine was seen once: {text}"
    );
    assert_eq!(seen(lines[2]), "4/4", "{text}");
    assert!(
        offers_table_marked(&[], 20, &OfferChanges::default(), &MarketWatch::default())
            .ends_with("(no offers)\n"),
        "an empty market"
    );
}

#[test]
fn a_machine_back_under_another_offer_id_has_no_mark_and_shows_its_old_price() {
    use vast_skopos::render::offers_table_marked;
    // Machine 1 comes back under another offer id and a new price; machine 2 is new.
    let before = [on_machine(10, 1, 0.5)];
    let now = [on_machine(11, 1, 0.55), on_machine(20, 2, 0.6)];
    let (watch, changes) = moved(&before, &now);
    let text = offers_table_marked(&now, 20, &changes, &watch);
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[1].starts_with("   11") && lines[1].contains("0.550 (was 0.500)"),
        "no + for a machine already known, and its old price: {text}"
    );
    assert!(lines[2].starts_with("+  20"), "{text}");
}

#[test]
fn every_row_of_a_new_machine_is_marked_but_only_the_row_that_moved_has_a_was() {
    use vast_skopos::render::offers_table_marked;
    // Machine 2 is new and answers twice in one look; machine 1 is repriced and answers twice, at
    // two prices: the watch follows its first row.
    let before = [on_machine(10, 1, 0.5)];
    let now = [
        on_machine(21, 2, 0.6),
        on_machine(22, 2, 0.7),
        on_machine(11, 1, 0.55),
        on_machine(12, 1, 0.9),
    ];
    let (watch, changes) = moved(&before, &now);
    let text = offers_table_marked(&now, 20, &changes, &watch);
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[1].starts_with("+  21") && lines[2].starts_with("+  22"),
        "{text}"
    );
    assert!(lines[3].contains("(was 0.500)"), "{text}");
    assert!(
        !lines[4].contains("was"),
        "no was for a row that did not move: {text}"
    );
}

#[test]
fn a_machine_the_watch_does_not_remember_has_a_dash_for_seen() {
    use vast_skopos::market::MarketWatch;
    use vast_skopos::offer::OfferChanges;
    use vast_skopos::render::offers_table_marked;
    let text = offers_table_marked(
        &[watched(1, 0.5)],
        20,
        &OfferChanges::default(),
        &MarketWatch::default(),
    );
    let row = text.lines().nth(1).unwrap();
    let seen = row.split_whitespace().rev().nth(2).unwrap();
    assert_eq!(seen, "-", "{text}");
}

#[test]
fn the_table_note_caps_its_gone_lines_but_changes_lists_every_change() {
    use vast_skopos::offer::OfferChanges;
    use vast_skopos::render::{offers_changes_note, offers_changes_only};
    let before: Vec<_> = (1..=13).map(|id| watched(id, 0.5)).collect();
    let mut c = OfferChanges::default();
    c.gone.extend(before);
    let note = offers_changes_note(&page(false), &c, &[], false);
    assert!(note.starts_with("since the last look: 13 gone\n"), "{note}");
    assert_eq!(
        note.lines().filter(|l| l.starts_with("- ")).count(),
        11,
        "ten, then the rest counted"
    );
    assert!(note.contains("- ... and 3 more\n"), "{note}");
    // A log must not lose ids: `--changes` names every one.
    let all = offers_changes_only(&page(false), Some(&c), &[], false, 0.0);
    assert_eq!(
        all.lines().filter(|l| l.starts_with("- ")).count(),
        13,
        "{all}"
    );
    assert!(!all.contains("more"), "{all}");
}

#[test]
fn the_cap_of_vast_ai_is_said_only_when_something_entered_or_left() {
    use vast_skopos::offer::OfferChanges;
    use vast_skopos::render::{offers_changes_note, offers_changes_only};
    let before: Vec<_> = (1..=3).map(|id| watched(id, 0.5)).collect();
    let mut gone = OfferChanges::default();
    gone.gone.extend(before.iter().cloned());
    assert!(!offers_changes_note(&page(false), &gone, &[], false).contains("caps a search"));
    assert!(
        offers_changes_note(&page(true), &gone, &[], false)
            .contains("vast.ai caps a search at 64 rows")
    );
    // Nothing entered or left: the cap explains nothing, and is not said.
    let steady = OfferChanges::default();
    assert!(!offers_changes_note(&page(true), &steady, &before, false).contains("caps a search"));
    assert!(
        !offers_changes_only(&page(true), Some(&steady), &before, false, 0.0)
            .contains("caps a search")
    );
    assert!(
        offers_changes_only(&page(true), Some(&gone), &[], false, 0.0).contains("caps a search")
    );
    assert!(
        offers_changes_only(&page(false), None, &before[..1], false, 0.0).starts_with(
            "searched 00:00:00 UTC: 1 offer (learning the market: changes are reported from look 4)"
        ),
        "singular, and the look from which changes are reported"
    );
}

#[test]
fn the_note_counts_the_new_and_repriced_offers_the_table_does_not_show() {
    use vast_skopos::render::offers_changes_note;
    let before = [watched(1, 0.5), watched(2, 0.6), watched(3, 0.7)];
    // Sorted by price, as the table shows them: 1, 2, 3, 4.
    let now = [
        watched(1, 0.5),
        watched(2, 0.65),
        watched(3, 0.7),
        watched(4, 0.9),
    ];
    let (_, c) = moved(&before, &now);
    let note =
        |shown: &[vast_skopos::offer::Offer]| offers_changes_note(&page(false), &c, shown, false);
    assert_eq!(note(&now), "since the last look: 1 new, 1 repriced\n");
    assert_eq!(
        note(&now[..2]),
        "since the last look: 1 new (1 below the table), 1 repriced\n",
        "the new offer is row 4, the repriced one row 2"
    );
    assert_eq!(
        note(&now[..1]),
        "since the last look: 1 new (1 below the table), 1 repriced (1 below the table)\n"
    );
    assert_eq!(
        note(&[]),
        "since the last look: 1 new (1 below the table), 1 repriced (1 below the table)\n",
        "a table of no rows shows nothing"
    );
}

#[test]
fn the_offers_below_the_table_are_counted_by_machine_not_by_offer_id() {
    use vast_skopos::render::offers_changes_note;
    let before = [on_machine(10, 1, 0.5)];
    let now = [on_machine(11, 1, 0.55)];
    let (_, c) = moved(&before, &now);
    // The table shows the same machine under yet another id: its row is on screen.
    let on_screen = [on_machine(12, 1, 0.55)];
    assert_eq!(
        offers_changes_note(&page(false), &c, &on_screen, false),
        "since the last look: 1 repriced\n"
    );
    assert_eq!(
        offers_changes_note(&page(false), &c, &[on_machine(20, 2, 0.9)], false),
        "since the last look: 1 repriced (1 below the table)\n"
    );
}

#[test]
fn a_search_says_that_its_answers_vary_and_a_watch_says_it_once_differently() {
    use vast_skopos::render::offers_variance_note;
    assert_eq!(
        offers_variance_note(false),
        "vast.ai's answers vary from one search to the next; repeat the search, or use --watch, to see more.\n"
    );
    let watching = offers_variance_note(true);
    assert!(watching.starts_with("vast.ai's answers vary from one search to the next; "));
    assert!(watching.contains("SEEN") && !watching.contains("repeat the search"));
    assert!(
        watching.contains(&format!(
            "from look {}",
            vast_skopos::market::LEARNING_LOOKS + 1
        )),
        "{watching}"
    );
}

#[test]
fn a_look_after_a_failed_search_says_so_in_both_modes() {
    use vast_skopos::render::{offers_changes_note, offers_changes_only};
    let before = [watched(1, 0.5)];
    let c = vast_skopos::offer::OfferChanges::default();
    assert_eq!(
        offers_changes_note(&page(false), &c, &before, true),
        "since the last successful look: no change\n"
    );
    assert_eq!(
        offers_changes_only(&page(false), Some(&c), &before, true, 0.0),
        "searched 00:00:00 UTC: 1 offer, no change (a search failed in between)\n"
    );
}

#[test]
fn a_credit_that_rounds_to_zero_has_no_sign_and_a_tiny_burn_no_infinite_runway() {
    let credit = |c: f64| Account::from_json(&serde_json::json!({"credit": c}));
    let running = |dph: f64| fleet_row(1, Some("running"), Some("running"), Some(dph));
    // A sliver of owed money rounds to $0.00, not $-0.00; a real debt keeps its sign.
    assert_eq!(
        money_line(&[], Some(&credit(-0.001))),
        "credit $0.00 | burn $0.000/h over 0 running\n"
    );
    assert!(money_line(&[], Some(&credit(-5.0))).starts_with("credit $-5.00 | "));
    // 1e-320 $/h is a burn that is not zero: five dollars would last `inf` hours.
    let line = money_line(&[running(1e-320)], Some(&credit(5.0)));
    assert!(
        line.starts_with("credit $5.00 | burn $0.000/h over 1 running") && !line.contains("left"),
        "{line}"
    );
    assert_eq!(
        vast_skopos::runway::Runway::of(&[running(1e-320)], Some(&credit(5.0))).hours_left(),
        None
    );
}

#[test]
fn a_repeated_id_is_said_to_be_one_not_blamed_on_a_constraint() {
    use vast_skopos::offer::select;
    // Four rows, one id twice; nothing constrains anything.
    let page = OfferPage::from_json(&serde_json::json!({"offers": [
        {"id": 1, "dph_total": 0.5}, {"id": 1, "dph_total": 0.6},
        {"id": 2, "dph_total": 0.7}, {"id": 3, "dph_total": 0.8}]}))
    .unwrap();
    let q = OfferQuery::default();
    let kept = select(page.offers.clone(), &q, OfferSort::Price);
    assert_eq!(kept.len(), 3);
    let footer = offers_footer(&page, &kept, 20, &q, OfferSort::Price, 0.0);
    assert!(
        footer.starts_with("3 offers, cheapest first | no constraint"),
        "{footer}"
    );
    assert!(footer.contains("1 repeated id skipped"), "{footer}");
    assert!(
        !footer.contains("dropped here"),
        "with no constraint nothing was dropped by one: {footer}"
    );
    // Two ids repeated, and one offer a constraint really drops: each is counted once, as itself.
    let page = OfferPage::from_json(&serde_json::json!({"offers": [
        {"id": 1, "geolocation": "Texas, US"}, {"id": 1, "geolocation": "Texas, US"},
        {"id": 2, "geolocation": "Texas, US"}, {"id": 2, "geolocation": "Texas, US"},
        {"id": 3, "geolocation": "Beijing, CN"}]}))
    .unwrap();
    let mut q = OfferQuery::default();
    q.exclude_countries = vec!["CN".to_owned()];
    let kept = select(page.offers.clone(), &q, OfferSort::Price);
    let footer = offers_footer(&page, &kept, 20, &q, OfferSort::Price, 0.0);
    assert!(
        footer
            .contains("2 repeated ids skipped | 1 dropped here: a field contradicts a constraint"),
        "{footer}"
    );
}

#[test]
fn a_debt_has_no_runway() {
    let owed = Account::from_json(&serde_json::json!({"credit": -5.0}));
    let running = fleet_row(1, Some("running"), Some("running"), Some(1.0));
    let line = money_line(&[running], Some(&owed));
    assert_eq!(
        line, "credit $-5.00 | burn $1.000/h over 1 running\n",
        "no `~-5.0 h left`"
    );
    assert_eq!(
        vast_skopos::runway::Runway::of(&[], Some(&owed)).hours_left(),
        None
    );
    // No credit left at all is no runway either; a positive credit still has one.
    let none = Account::from_json(&serde_json::json!({"credit": 0.0}));
    let running = fleet_row(1, Some("running"), Some("running"), Some(1.0));
    assert!(!money_line(std::slice::from_ref(&running), Some(&none)).contains("left"));
    let some = Account::from_json(&serde_json::json!({"credit": 3.0}));
    assert!(money_line(&[running], Some(&some)).ends_with("~3.0 h left at this rate\n"));
}

#[test]
fn the_trend_is_a_column_after_the_vram_and_only_when_a_watch_has_one() {
    use vast_skopos::probe::ProbeOutcome;
    use vast_skopos::trend::Trend;
    let rows = vec![
        instance(&serde_json::json!({"id": 1, "actual_status": "running"})),
        instance(&serde_json::json!({"id": 2, "actual_status": "exited"})),
    ];
    let probe = |util: u32| -> ProbeOutcome {
        Some(Ok(parse_output(&format!(
            "0, {util}, 100, 200\n---vas-log---\nvas-no-file\n"
        ))))
    };
    let mut trend = Trend::default();
    for util in [97, 99, 0, 98] {
        trend.record(&rows, &[probe(util), None]);
    }
    let words = |text: &str, id: &str| -> Vec<String> {
        text.lines()
            .find(|l| l.starts_with(id))
            .unwrap()
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    };
    let with = table(
        &rows,
        &[probe(98), None],
        None,
        TableOptions::new().ssh(true).trend(&trend),
        0.0,
    );
    let header: Vec<&str> = with.lines().next().unwrap().split_whitespace().collect();
    let at = header.iter().position(|h| *h == "VRAM").unwrap();
    assert_eq!(
        &header[at..at + 8],
        [
            "VRAM", "POWER", "CLOCK", "TREND", "CLOCK", "TREND", "LAST", "LOG"
        ],
        "{with}"
    );
    let running = words(&with, "1");
    assert!(
        running.windows(4).any(|w| w == ["97", "99", "0", "98"]),
        "the four samples, oldest first: {with}"
    );
    assert!(
        words(&with, "2").contains(&"-".to_owned()),
        "a box with none: {with}"
    );
    // Without a trend, or without the probe columns, there is no such column.
    let plain = table(
        &rows,
        &[probe(98), None],
        None,
        TableOptions::new().ssh(true),
        0.0,
    );
    assert!(!plain.contains("TREND"), "{plain}");
    let quiet = table(&rows, &[], None, TableOptions::new().trend(&trend), 0.0);
    assert!(
        !quiet.contains("TREND") && !quiet.contains("VRAM"),
        "{quiet}"
    );
}

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
use vast_skopos::render::{offers_line, offers_table, table};

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
    let text = table(&rows, &probes, true, Some(&account), now);
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
    let text = table(&[], &[], false, None, 0.0);
    assert!(
        text.contains("(no instances)") && text.contains("credit ? | burn $0.000/h over 0 running")
    );
}

#[test]
fn ssh_columns_show_when_asked_even_with_nothing_probed() {
    let rows = vec![instance(
        &serde_json::json!({"id": 1, "actual_status": "exited"}),
    )];
    let text = table(&rows, &[None], true, None, 0.0);
    assert!(text.contains("GPU% (ssh)"), "{text}");
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
    let text = table(&rows, &probes, true, None, 0.0);
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
        offers_line(&page, kept.len(), 2, &q, OfferSort::Price),
        "2 of 3 offers shown, cheapest first | verified | $/h includes 5 GB of storage\n\
         2 dropped here: a field contradicts a constraint\n"
    );
}

#[test]
fn an_answer_at_the_cap_says_the_list_may_be_cut() {
    let rows: Vec<serde_json::Value> = (0..64).map(|id| serde_json::json!({"id": id})).collect();
    let page =
        OfferPage::from_json(&serde_json::json!({"offers": rows, "truncated": false})).unwrap();
    let footer = offers_line(&page, 64, 20, &OfferQuery::default(), OfferSort::Price);
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
    let footer = offers_line(&page, 1, 1, &OfferQuery::default(), OfferSort::Price);
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

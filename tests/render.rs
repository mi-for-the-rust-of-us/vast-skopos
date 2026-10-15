//! The table, from fixtures.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

use vast_skopos::SkoposError;
use vast_skopos::instance::{Account, Instance};
use vast_skopos::probe::parse_output;
use vast_skopos::render::table;

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
            "0, 98, 30122, 32607\n---vas-log---\n[ 796.4s] layer 4: ...\n",
        ))),
        Some(Err(SkoposError::Ssh {
            target: "h:1".to_owned(),
            message: "Connection refused".to_owned(),
        })),
    ];
    let account = Account::from_json(&serde_json::json!({"credit": 5.94}));
    let text = table(&rows, &probes, Some(&account), now);
    assert!(text.contains("1x RTX 5090"), "{text}");
    assert!(
        text.contains("2h 0m") && text.contains("1.29"),
        "age and cost: {text}"
    );
    assert!(
        text.contains("98%") && text.contains("29.4/32G"),
        "probe: {text}"
    );
    assert!(text.contains("layer 4"), "log line: {text}");
    assert!(text.contains("Connection refused"), "ssh error: {text}");
    assert!(
        text.contains("credit $5.94 | burn $0.643/h over 1 running | ~9.2 h left"),
        "{text}"
    );
}

#[test]
fn no_instances_still_prints_the_money_line() {
    let text = table(&[], &[], None, 0.0);
    assert!(
        text.contains("(no instances)") && text.contains("credit ? | burn $0.000/h over 0 running")
    );
}

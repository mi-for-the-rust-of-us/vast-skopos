//! The client-side re-check and order, constraint by constraint: each one drops the row whose
//! field contradicts it, keeps the row missing that field, and is sent to the server in the same
//! unit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

use serde_json::{Value, json};
use vast_skopos::offer::{Offer, OfferQuery, OfferSort, select};

/// Every constraint `vas offers` has, each set.
fn every_constraint() -> OfferQuery {
    let mut q = OfferQuery::default();
    q.gpu = Some("RTX 5090".to_owned());
    q.num_gpus = Some(1);
    q.disk = Some(50);
    q.cuda = Some(12.8);
    q.reliability = Some(0.99);
    q.inet_up = Some(200.0);
    q.inet_down = Some(300.0);
    q.max_price = Some(0.8);
    q.min_days = Some(2.0);
    q.verified = true;
    q.exclude_countries = vec!["CN".to_owned()];
    q.exclude_hosts = vec![362_355];
    q.exclude_machines = vec![142_875];
    q
}

/// A row passing every constraint of [`every_constraint`].
fn passing() -> Value {
    json!({"id": 1, "gpu_name": "RTX 5090", "num_gpus": 1, "disk_space": 50.0,
      "cuda_max_good": 12.8, "reliability2": 0.99, "inet_up": 200.0, "inet_down": 300.0,
      "dph_total": 0.8, "duration": 172_800.0, "verification": "verified",
      "geolocation": "Texas, US", "host_id": 7, "machine_id": 8, "rentable": true, "rented": false})
}

/// Each field, and a value contradicting its constraint (bounds are inclusive in `passing`).
const VIOLATIONS: [(&str, &str); 15] = [
    ("gpu_name", r#""RTX 4090""#),
    ("num_gpus", "2"),
    ("disk_space", "49.9"),
    ("cuda_max_good", "12.7"),
    ("reliability2", "0.989"),
    ("inet_up", "199.9"),
    ("inet_down", "299.9"),
    ("dph_total", "0.801"),
    ("duration", "172799.0"),
    ("verification", r#""deverified""#),
    ("geolocation", r#""Guangdong, CN""#),
    ("host_id", "362355"),
    ("machine_id", "142875"),
    ("rentable", "false"),
    ("rented", "true"),
];

#[test]
fn each_constraint_alone_drops_a_contradicting_row_and_keeps_a_missing_field() {
    let q = every_constraint();
    let offer = |row: &Value| Offer::from_json(row).unwrap();
    assert!(q.admits(&offer(&passing())), "the bounds are inclusive");
    for (field, bad) in VIOLATIONS {
        let mut contradicting = passing();
        contradicting[field] = serde_json::from_str(bad).unwrap();
        assert!(
            !q.admits(&offer(&contradicting)),
            "{field} = {bad} must drop the row"
        );
        let mut missing = passing();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            q.admits(&offer(&missing)),
            "{field} missing must keep the row"
        );
    }
}

#[test]
fn every_constraint_is_sent_in_the_servers_unit() {
    let q = every_constraint().to_q(OfferSort::Price);
    for (field, want) in [
        ("gpu_name", json!({"eq": "RTX 5090"})),
        ("num_gpus", json!({"eq": 1})),
        ("disk_space", json!({"gte": 50})),
        ("cuda_max_good", json!({"gte": 12.8})),
        ("reliability2", json!({"gte": 0.99})),
        ("inet_up", json!({"gte": 200.0})),
        ("inet_down", json!({"gte": 300.0})),
        ("dph_total", json!({"lte": 0.8})),
        ("duration", json!({"gte": 172_800.0})),
        ("verified", json!({"eq": true})),
        ("geolocation", json!({"notin": ["CN"]})),
        ("host_id", json!({"notin": [362_355]})),
        ("machine_id", json!({"notin": [142_875]})),
        ("rentable", json!({"eq": true})),
        ("rented", json!({"eq": false})),
        ("allocated_storage", json!(50)),
    ] {
        assert_eq!(q[field], want, "{field}");
    }
}

#[test]
fn the_gpu_model_and_the_countries_ignore_ascii_case() {
    let mut q = OfferQuery::default();
    q.gpu = Some("rtx 5090".to_owned());
    q.exclude_countries = vec!["cn".to_owned()];
    let row = |g: &str, at: &str| {
        Offer::from_json(&json!({"id": 1, "gpu_name": g, "geolocation": at})).unwrap()
    };
    assert!(q.admits(&row("RTX 5090", "Texas, US")));
    assert!(!q.admits(&row("RTX 5090", "Guangdong, CN")));
    assert_eq!(
        q.to_q(OfferSort::Price)["geolocation"],
        json!({"notin": ["CN"]})
    );
}

#[test]
fn every_order_puts_missing_values_last_and_breaks_ties_by_price_then_id() {
    let rows = [
        json!({"id": 5, "dph_total": 0.5, "reliability2": 0.99, "inet_up": 900.0}),
        json!({"id": 4, "dph_total": 0.4}),
        json!({"id": 3, "dph_total": 0.6, "reliability2": 0.99, "inet_up": 900.0}),
        json!({"id": 2, "dph_total": 0.5, "reliability2": 0.999, "inet_up": 100.0}),
        json!({"id": 1, "reliability2": 0.95, "inet_up": 100.0}),
    ];
    let offers: Vec<Offer> = rows.iter().map(|r| Offer::from_json(r).unwrap()).collect();
    let ids = |sort| -> Vec<u64> {
        select(offers.clone(), &OfferQuery::default(), sort)
            .iter()
            .map(|o| o.id)
            .collect()
    };
    assert_eq!(ids(OfferSort::Price), [4, 2, 5, 3, 1]);
    assert_eq!(ids(OfferSort::Reliability), [2, 5, 3, 1, 4]);
    assert_eq!(ids(OfferSort::Up), [5, 3, 2, 1, 4]);
}

#[test]
fn a_host_and_a_machine_are_told_apart() {
    // The ids are drawn from the same range, so a host can be numbered like another host's machine.
    let offer = Offer::from_json(&json!({"id": 1, "host_id": 5, "machine_id": 9})).unwrap();
    assert_eq!((offer.host_id, offer.machine_id), (Some(5), Some(9)));
    let admits = |hosts: &[u64], machines: &[u64]| {
        let mut q = OfferQuery::default();
        q.exclude_hosts = hosts.to_vec();
        q.exclude_machines = machines.to_vec();
        q.admits(&offer)
    };
    assert!(!admits(&[5], &[]), "its host");
    assert!(!admits(&[], &[9]), "its machine");
    assert!(!admits(&[5], &[9]), "both");
    assert!(
        admits(&[9], &[]),
        "a host numbered like its machine is another host"
    );
    assert!(
        admits(&[], &[5]),
        "a machine numbered like its host is another machine"
    );
    assert!(admits(&[4, 6], &[8, 10]), "neither");

    let mut q = OfferQuery::default();
    q.exclude_hosts = vec![5];
    q.exclude_machines = vec![9];
    let sent = q.to_q(OfferSort::Price);
    assert_eq!(sent["host_id"], json!({"notin": [5]}));
    assert_eq!(sent["machine_id"], json!({"notin": [9]}));
    assert!(
        q.summary().ends_with("not hosts 5, not machines 9"),
        "{}",
        q.summary()
    );
}

/// `(id, $/h)` offers, in that order.
fn priced(rows: &[(u64, Option<f64>)]) -> Vec<Offer> {
    rows.iter()
        .map(|&(id, price)| {
            let mut row = json!({"id": id});
            if let Some(p) = price {
                row["dph_total"] = json!(p);
            }
            Offer::from_json(&row).unwrap()
        })
        .collect()
}

#[test]
fn the_changes_name_what_appeared_what_was_repriced_and_what_is_gone() {
    let before = priced(&[
        (1, Some(0.50)),
        (2, Some(0.60)),
        (3, Some(0.70)),
        (4, Some(0.80)),
    ]);
    let now = priced(&[
        (9, Some(0.40)),
        (1, Some(0.55)),
        (3, Some(0.70)),
        (8, Some(0.90)),
    ]);
    let c = vast_skopos::offer::changes(&before, &now);
    let ids = |offers: &[Offer]| offers.iter().map(|o| o.id).collect::<Vec<_>>();
    assert_eq!(ids(&c.appeared), [9, 8], "in the current look's order");
    assert_eq!(ids(&c.gone), [2, 4], "in the previous look's order");
    assert_eq!(c.repriced.len(), 1);
    assert_eq!((c.repriced[0].offer.id, c.repriced[0].was), (1, 0.50));
    assert_eq!(
        c.repriced[0].offer.dph_total,
        Some(0.55),
        "the price as it reads now"
    );
    assert!(!c.is_empty());
}

#[test]
fn nothing_moved_when_the_looks_agree_to_the_shown_price() {
    let before = priced(&[
        (1, Some(0.5000)),
        (2, Some(0.6004)),
        (3, None),
        (4, Some(0.7)),
    ]);
    // 0.6004 -> 0.6001: the same 0.600; a price missing on either side cannot be said to move.
    let now = priced(&[
        (4, Some(0.7)),
        (3, Some(0.9)),
        (2, Some(0.6001)),
        (1, Some(0.5)),
    ]);
    assert!(
        vast_skopos::offer::changes(&before, &now).is_empty(),
        "a different order is not a change"
    );
    assert!(vast_skopos::offer::changes(&[], &[]).is_empty());
    // Rounding is of what the table prints: 0.5004 and 0.5006 print as 0.500 and 0.501.
    let c =
        vast_skopos::offer::changes(&priced(&[(1, Some(0.5004))]), &priced(&[(1, Some(0.5006))]));
    assert_eq!(c.repriced.len(), 1);
}

#[test]
fn an_offer_that_sank_in_the_ranking_is_not_gone_and_the_first_look_has_everything_new() {
    // The comparison is of the full lists kept: `vas` passes those, not the rows shown.
    let all = priced(&[(1, Some(0.5)), (2, Some(0.6)), (3, Some(0.7))]);
    let reordered = priced(&[(3, Some(0.7)), (2, Some(0.6)), (1, Some(0.5))]);
    assert!(vast_skopos::offer::changes(&all, &reordered).is_empty());
    let first = vast_skopos::offer::changes(&[], &all);
    assert_eq!((first.appeared.len(), first.gone.len()), (3, 0));
}

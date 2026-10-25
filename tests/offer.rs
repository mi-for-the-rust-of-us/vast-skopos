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
    q.exclude_deverified = true;
    q.exclude_countries = vec!["CN".to_owned()];
    q.only_hosts = vec![7];
    q.only_machines = vec![8];
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
const VIOLATIONS: [(&str, &str); 17] = [
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
    ("host_id", "9"),
    ("machine_id", "142875"),
    ("machine_id", "10"),
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
    let q = every_constraint().to_search_json(OfferSort::Price);
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
        ("host_id", json!({"in": [7], "notin": [362_355]})),
        ("machine_id", json!({"in": [8], "notin": [142_875]})),
        ("rentable", json!({"eq": true})),
        ("rented", json!({"eq": false})),
        ("allocated_storage", json!(50)),
    ] {
        assert_eq!(q[field], want, "{field}");
    }
    assert!(
        q.get("vericode").is_none(),
        "`verified` already leaves the deverified out, and the server applies only one of the two"
    );
}

#[test]
fn excluding_the_deverified_keeps_the_unverified_and_is_sent_as_vericode() {
    let mut q = OfferQuery::default();
    q.exclude_deverified = true;
    assert!(!q.is_identity());
    let at = |verification: Option<&str>| {
        let mut row = json!({"id": 1});
        if let Some(v) = verification {
            row["verification"] = v.into();
        }
        q.admits(&Offer::from_json(&row).unwrap())
    };
    assert!(!at(Some("deverified")));
    assert!(at(Some("unverified")), "new machines stay");
    assert!(at(Some("verified")));
    assert!(at(None), "a missing field keeps the row");
    assert_eq!(
        q.to_search_json(OfferSort::Price)["vericode"],
        json!({"lte": 1})
    );
    assert_eq!(q.summary(), "not deverified");
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
        q.to_search_json(OfferSort::Price)["geolocation"],
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
    let sent = q.to_search_json(OfferSort::Price);
    assert_eq!(sent["host_id"], json!({"notin": [5]}));
    assert_eq!(sent["machine_id"], json!({"notin": [9]}));
    assert!(
        q.summary().ends_with("not hosts 5, not machines 9"),
        "{}",
        q.summary()
    );
}

#[test]
fn only_the_hosts_and_machines_asked_for_are_kept_and_an_excluded_one_still_goes() {
    let offer = |host: Option<u64>, machine: Option<u64>| {
        let mut row = json!({"id": 1});
        if let Some(h) = host {
            row["host_id"] = h.into();
        }
        if let Some(m) = machine {
            row["machine_id"] = m.into();
        }
        Offer::from_json(&row).unwrap()
    };
    let mut q = OfferQuery::default();
    q.only_hosts = vec![406_325, 102_965, 406_325];
    assert!(!q.is_identity());
    assert!(q.admits(&offer(Some(406_325), None)));
    assert!(!q.admits(&offer(Some(155_385), None)), "another host");
    assert!(
        q.admits(&offer(None, None)),
        "a missing field keeps the row"
    );
    q.exclude_hosts = vec![102_965];
    assert!(!q.admits(&offer(Some(102_965), None)), "excluded wins");
    q.only_machines = vec![145_061];
    assert!(q.admits(&offer(Some(406_325), Some(145_061))));
    assert!(
        !q.admits(&offer(Some(406_325), Some(142_476))),
        "a sibling machine"
    );
    let sent = q.to_search_json(OfferSort::Price);
    assert_eq!(
        sent["host_id"],
        json!({"in": [102_965, 406_325], "notin": [102_965]})
    );
    assert_eq!(sent["machine_id"], json!({"in": [145_061]}));
    assert_eq!(
        q.summary(),
        "hosts 102965 406325, machines 145061, not hosts 102965"
    );
}

#[test]
fn an_id_answered_twice_is_one_offer_and_the_row_that_sorts_first_stays() {
    let rows = [(7, 0.9, 0.5), (3, 0.6, 0.9), (7, 0.5, 0.2), (3, 0.6, 0.95)];
    let offers: Vec<Offer> = rows
        .iter()
        .map(|&(id, price, reliability)| {
            Offer::from_json(&json!({"id": id, "dph_total": price, "reliability2": reliability}))
                .unwrap()
        })
        .collect();
    let by_price = select(offers.clone(), &OfferQuery::default(), OfferSort::Price);
    assert_eq!(
        by_price
            .iter()
            .map(|o| (o.id, o.dph_total))
            .collect::<Vec<_>>(),
        [(7, Some(0.5)), (3, Some(0.6))],
        "the cheaper 7; the first of two equal 3s"
    );
    let by_reliability = select(offers, &OfferQuery::default(), OfferSort::Reliability);
    assert_eq!(
        by_reliability
            .iter()
            .map(|o| (o.id, o.reliability))
            .collect::<Vec<_>>(),
        [(3, Some(0.95)), (7, Some(0.5))],
        "the most reliable row of each id, since that is how the list is ordered"
    );
}

//! What a watch makes of searches that disagree: machines, not offer ids, over several looks.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::assert_is_empty)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion; and `is_empty()` reads better than
// a comparison with an empty array of offers.

use vast_skopos::market::{ABSENT_FOR_GONE, ABSENT_FOR_NEW, LEARNING_LOOKS, MarketWatch, OfferKey};
use vast_skopos::offer::{Offer, OfferChanges};

/// An offer of `machine` with `gpus` GPUs, under the offer `id`, at `price`.
fn offer(id: u64, machine: u64, gpus: u64, price: f64) -> Offer {
    Offer::from_json(&serde_json::json!(
        {"id": id, "machine_id": machine, "num_gpus": gpus, "dph_total": price}))
    .unwrap()
}

/// The offer of `machine` (one GPU) at the id `machine * 10`.
fn at(machine: u64, price: f64) -> Offer {
    offer(machine * 10, machine, 1, price)
}

fn machines(offers: &[Offer]) -> Vec<u64> {
    offers.iter().filter_map(|o| o.machine_id).collect()
}

/// A watch that has done its learning looks of `offers`.
fn learned(offers: &[Offer]) -> MarketWatch {
    let mut watch = MarketWatch::default();
    for _ in 0..LEARNING_LOOKS {
        assert!(watch.record(offers).is_none());
    }
    watch
}

#[test]
fn the_first_looks_only_learn_and_the_next_one_reports() {
    let mut watch = MarketWatch::default();
    assert_eq!(watch.looks(), 0);
    for look in 1..=LEARNING_LOOKS {
        assert!(
            watch.record(&[at(1, 0.5), at(2, 0.6)]).is_none(),
            "look {look} is still learning"
        );
        assert_eq!(watch.looks(), look);
    }
    let report = watch.record(&[at(1, 0.5), at(2, 0.6)]).unwrap();
    assert!(report.is_empty(), "the machines learned are not new");
    assert_eq!(watch.looks(), LEARNING_LOOKS + 1);
}

#[test]
fn a_machine_that_appears_after_the_learning_is_new_once() {
    let mut watch = learned(&[at(1, 0.5)]);
    let report = watch.record(&[at(1, 0.5), at(2, 0.6)]).unwrap();
    assert_eq!(machines(&report.appeared), [2]);
    let again = watch.record(&[at(1, 0.5), at(2, 0.6)]).unwrap();
    assert!(again.is_empty(), "new on the look it appears, not after");
}

#[test]
fn the_same_machine_under_another_offer_id_is_neither_new_nor_gone() {
    // The ids of one machine alternate from one answer to the next (measured 2026-10-18).
    let mut watch = learned(&[offer(100, 7, 1, 0.5)]);
    for id in [101, 102, 100, 103, 100, 104, 100] {
        let report = watch.record(&[offer(id, 7, 1, 0.5)]).unwrap();
        assert!(report.is_empty(), "id {id}: {report:?}");
    }
}

#[test]
fn a_machine_is_told_apart_by_its_gpu_count_and_an_anonymous_offer_by_its_id() {
    assert_ne!(
        OfferKey::of(&offer(1, 7, 1, 0.5)),
        OfferKey::of(&offer(1, 7, 4, 0.5)),
        "one GPU and four of the same machine are two offers"
    );
    let anonymous = |id: u64| Offer::from_json(&serde_json::json!({"id": id})).unwrap();
    assert_eq!(OfferKey::of(&anonymous(9)), OfferKey::of(&anonymous(9)));
    assert_ne!(OfferKey::of(&anonymous(9)), OfferKey::of(&anonymous(10)));
    let mut watch = learned(&[offer(1, 7, 1, 0.5)]);
    let report = watch
        .record(&[offer(1, 7, 1, 0.5), offer(2, 7, 4, 2.0)])
        .unwrap();
    assert_eq!(report.appeared.len(), 1);
    assert_eq!(report.appeared[0].num_gpus, Some(4));
}

#[test]
fn a_machine_that_blinks_is_not_new_until_it_was_absent_long_enough() {
    // Absent for ABSENT_FOR_NEW - 1 looks, then back: a blink.
    let mut watch = learned(&[at(1, 0.5), at(2, 0.6)]);
    for _ in 0..ABSENT_FOR_NEW - 1 {
        assert!(watch.record(&[at(1, 0.5)]).unwrap().is_empty());
    }
    let back = watch.record(&[at(1, 0.5), at(2, 0.6)]).unwrap();
    assert!(back.appeared.is_empty(), "a blink: {back:?}");

    // Absent for ABSENT_FOR_NEW looks (still short of gone), then back: new.
    let mut watch = learned(&[at(1, 0.5), at(2, 0.6)]);
    for _ in 0..ABSENT_FOR_NEW {
        assert!(watch.record(&[at(1, 0.5)]).unwrap().is_empty());
    }
    let back = watch.record(&[at(1, 0.5), at(2, 0.6)]).unwrap();
    assert_eq!(machines(&back.appeared), [2]);
}

#[test]
fn a_machine_is_gone_after_five_looks_missing_in_a_row_and_said_once() {
    let mut watch = learned(&[at(1, 0.5), at(2, 0.6)]);
    for look in 1..ABSENT_FOR_GONE {
        let report = watch.record(&[at(1, 0.5)]).unwrap();
        assert!(report.gone.is_empty(), "missing {look} looks: not yet");
    }
    let report = watch.record(&[at(1, 0.5)]).unwrap();
    assert_eq!(
        machines(&report.gone),
        [2],
        "missing {ABSENT_FOR_GONE} looks"
    );
    assert!(watch.record(&[at(1, 0.5)]).unwrap().is_empty(), "said once");
    assert_eq!(watch.seen(&at(2, 0.6)), None, "forgotten");
    // Back after being gone: new again.
    let report = watch.record(&[at(1, 0.5), at(2, 0.6)]).unwrap();
    assert_eq!(machines(&report.appeared), [2]);
    assert_eq!(
        watch.seen(&at(2, 0.6)),
        Some((1, 1)),
        "its sightings start afresh"
    );
}

#[test]
fn a_machine_that_blinks_back_in_time_is_not_gone() {
    let mut watch = learned(&[at(1, 0.5), at(2, 0.6)]);
    for _ in 0..ABSENT_FOR_GONE - 1 {
        assert!(watch.record(&[at(1, 0.5)]).unwrap().gone.is_empty());
    }
    // Missing four looks, back on the fifth: never gone (and new, having been away for long).
    let back = watch.record(&[at(1, 0.5), at(2, 0.6)]).unwrap();
    assert!(back.gone.is_empty());
    assert_eq!(machines(&back.appeared), [2]);
}

#[test]
fn the_gone_come_cheapest_first_then_by_id() {
    let all = [at(1, 0.9), at(2, 0.5), offer(31, 3, 1, 0.5), at(4, 0.7)];
    let mut watch = learned(&all);
    let mut report = OfferChanges::default();
    for _ in 0..ABSENT_FOR_GONE {
        report = watch.record(&[]).unwrap();
    }
    let ids: Vec<u64> = report.gone.iter().map(|o| o.id).collect();
    assert_eq!(ids, [20, 31, 40, 10]);
}

#[test]
fn a_price_that_moves_is_compared_with_the_last_price_seen_to_the_thousandth() {
    let mut watch = learned(&[at(1, 0.5000), at(2, 0.6004)]);
    // 0.6004 -> 0.6001 is the same 0.600; 0.5 -> 0.55 moved.
    let report = watch.record(&[at(1, 0.55), at(2, 0.6001)]).unwrap();
    assert_eq!(report.repriced.len(), 1);
    assert_eq!(
        (report.repriced[0].offer.id, report.repriced[0].was),
        (10, 0.5)
    );
    assert_eq!(report.repriced[0].offer.dph_total, Some(0.55));
    // The next look compares with 0.55, not with the price of the learning looks.
    let report = watch.record(&[at(1, 0.60), at(2, 0.6001)]).unwrap();
    assert!((report.repriced[0].was - 0.55).abs() < 1e-12);
    assert!(watch.record(&[at(1, 0.60)]).unwrap().repriced.is_empty());
    // 0.5004 and 0.5006 print as 0.500 and 0.501: a move.
    let mut watch = learned(&[at(1, 0.5004)]);
    assert_eq!(watch.record(&[at(1, 0.5006)]).unwrap().repriced.len(), 1);
}

#[test]
fn a_missing_price_cannot_be_said_to_move() {
    let unpriced = Offer::from_json(&serde_json::json!(
        {"id": 10, "machine_id": 1, "num_gpus": 1}))
    .unwrap();
    let mut watch = learned(std::slice::from_ref(&unpriced));
    assert!(watch.record(&[at(1, 0.9)]).unwrap().is_empty());
    assert!(watch.record(&[unpriced]).unwrap().is_empty());
}

#[test]
fn a_machine_back_after_a_long_absence_is_new_not_repriced() {
    let mut watch = learned(&[at(1, 0.5), at(2, 0.6)]);
    for _ in 0..ABSENT_FOR_NEW {
        watch.record(&[at(1, 0.5)]).unwrap();
    }
    let back = watch.record(&[at(1, 0.5), at(2, 0.9)]).unwrap();
    assert_eq!(machines(&back.appeared), [2]);
    assert!(back.repriced.is_empty(), "{back:?}");
}

#[test]
fn a_machine_answered_twice_in_one_look_counts_once_and_the_cheaper_row_stays() {
    let mut watch = learned(&[at(1, 0.5)]);
    let report = watch
        .record(&[offer(22, 2, 1, 0.9), offer(21, 2, 1, 0.4), at(1, 0.5)])
        .unwrap();
    assert_eq!(report.appeared.len(), 1);
    assert_eq!(
        report.appeared[0].id, 21,
        "the cheaper row, not the first one: the order of --sort must not decide"
    );
    assert_eq!(
        watch.seen(&at(2, 0.4)),
        Some((1, 1)),
        "one sighting, not two"
    );
    // The same price twice: the lower id.
    let report = watch
        .record(&[
            offer(32, 3, 1, 0.7),
            offer(31, 3, 1, 0.7),
            at(1, 0.5),
            at(2, 0.4),
        ])
        .unwrap();
    assert_eq!(report.appeared[0].id, 31);
    // The kept row does not flip from look to look, so no price seems to move.
    let steady = watch
        .record(&[
            offer(31, 3, 1, 0.7),
            offer(32, 3, 1, 0.9),
            at(1, 0.5),
            at(2, 0.4),
        ])
        .unwrap();
    assert!(steady.repriced.is_empty(), "{steady:?}");
}

#[test]
fn a_look_that_gave_no_price_does_not_erase_the_last_price_seen() {
    let unpriced = Offer::from_json(&serde_json::json!(
        {"id": 10, "machine_id": 1, "num_gpus": 1}))
    .unwrap();
    let mut watch = learned(&[at(1, 0.5)]);
    assert!(
        watch
            .record(std::slice::from_ref(&unpriced))
            .unwrap()
            .is_empty()
    );
    let report = watch.record(&[at(1, 0.6)]).unwrap();
    assert_eq!(
        report.repriced.len(),
        1,
        "0.5 -> (nothing) -> 0.6 is a move"
    );
    assert!((report.repriced[0].was - 0.5).abs() < 1e-12);
}

#[test]
fn offers_without_a_machine_are_followed_by_their_id() {
    let anonymous = |id: u64, price: f64| {
        Offer::from_json(&serde_json::json!({"id": id, "dph_total": price})).unwrap()
    };
    let mut watch = learned(&[anonymous(1, 0.5)]);
    let report = watch
        .record(&[anonymous(1, 0.55), anonymous(2, 0.6)])
        .unwrap();
    assert_eq!(report.appeared.len(), 1);
    assert_eq!(report.appeared[0].id, 2);
    assert_eq!(report.repriced.len(), 1);
    assert_eq!(watch.seen(&anonymous(1, 0.55)), Some((4, 4)));
}

#[test]
fn a_gone_machine_with_no_price_comes_last() {
    let unpriced = Offer::from_json(&serde_json::json!(
        {"id": 5, "machine_id": 5, "num_gpus": 1}))
    .unwrap();
    let mut watch = learned(&[unpriced, at(1, 0.9)]);
    let mut report = OfferChanges::default();
    for _ in 0..ABSENT_FOR_GONE {
        report = watch.record(&[]).unwrap();
    }
    let ids: Vec<u64> = report.gone.iter().map(|o| o.id).collect();
    assert_eq!(ids, [10, 5]);
}

#[test]
fn seen_counts_the_looks_a_machine_appeared_in_out_of_the_looks_since_its_first() {
    let mut watch = MarketWatch::default();
    assert_eq!(watch.seen(&at(1, 0.5)), None, "nothing is remembered yet");
    watch.record(&[at(1, 0.5)]);
    watch.record(&[at(1, 0.5), at(2, 0.6)]);
    watch.record(&[at(2, 0.6)]);
    watch.record(&[at(1, 0.5), at(2, 0.6)]);
    assert_eq!(watch.seen(&at(1, 0.5)), Some((3, 4)), "3 of the 4 looks");
    assert_eq!(
        watch.seen(&at(2, 0.6)),
        Some((3, 3)),
        "3 of the 3 since its first"
    );
    // A machine seen again after a long absence starts afresh.
    for _ in 0..ABSENT_FOR_NEW {
        watch.record(&[at(2, 0.6)]);
    }
    watch.record(&[at(1, 0.5), at(2, 0.6)]);
    assert_eq!(watch.seen(&at(1, 0.5)), Some((1, 1)));
}

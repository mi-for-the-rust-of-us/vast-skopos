//! What a watch makes of searches that do not agree with each other.
//!
//! vast.ai's search answers a SAMPLE of the offers that match: measured on 2026-10-18, fifteen
//! identical searches 2.5 s apart saw 129 machines, 64 per answer, and only 30 of them in every
//! answer; the others came and went, evenly, and the same machine could show up under two or three
//! offer ids. Comparing one answer with the next, by offer id, reports that sampling as a market
//! (half the list "new" and half "gone", on every look).
//!
//! A [`MarketWatch`] therefore remembers the machines over SEVERAL looks:
//!
//! - an offer is identified by its machine and GPU count ([`OfferKey`]), not by its id;
//! - the first [`LEARNING_LOOKS`] looks only learn: nothing is reported;
//! - after that, a machine is new when it was absent from the previous [`ABSENT_FOR_NEW`] looks, and
//!   gone when it has been missing for [`ABSENT_FOR_GONE`] looks in a row (reported once);
//! - a price that moved is compared with the last price seen;
//! - [`MarketWatch::seen`] says in how many looks a machine appeared, which is the flicker, made
//!   visible: a machine seen in every look is a dependable listing, one seen in a third of them
//!   may be a host that reports late (a hypothesis, not a finding).

use std::collections::{HashMap, HashSet};

use crate::offer::{Offer, OfferChanges, Repriced};

/// The looks that only learn the market: nothing is reported before the next one.
pub const LEARNING_LOOKS: usize = 3;

/// A machine absent from this many looks in a row, then seen again, is new.
pub const ABSENT_FOR_NEW: usize = 3;

/// A machine missing for this many looks in a row is gone.
pub const ABSENT_FOR_GONE: usize = 5;

/// What identifies an offer across looks: its machine and how many GPUs it offers (the offer id
/// alternates, for one machine, from one answer to the next); the id itself when the machine is
/// not given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OfferKey {
    /// A machine's offer of this many GPUs.
    Machine {
        /// The machine's id.
        machine_id: u64,
        /// How many GPUs the offer has (0 when the answer did not say).
        num_gpus: u64,
    },
    /// An offer whose machine is not known: the offer id.
    Offer(u64),
}

impl OfferKey {
    /// The key of `offer`.
    ///
    /// ```
    /// use vast_skopos::{market::OfferKey, offer::Offer};
    /// let at = |id: u64| Offer::from_json(&serde_json::json!(
    ///   {"id": id, "machine_id": 7, "num_gpus": 1})).unwrap();
    /// assert_eq!(OfferKey::of(&at(1)), OfferKey::of(&at(2)), "the same machine under two ids");
    /// let anonymous = Offer::from_json(&serde_json::json!({"id": 9})).unwrap();
    /// assert_eq!(OfferKey::of(&anonymous), OfferKey::Offer(9));
    /// ```
    #[must_use]
    pub fn of(offer: &Offer) -> Self {
        offer
            .machine_id
            .map_or(Self::Offer(offer.id), |machine_id| Self::Machine {
                machine_id,
                num_gpus: offer.num_gpus.unwrap_or(0),
            })
    }
}

/// What a watch remembers of one machine's offer.
#[derive(Debug, Clone)]
struct Entry {
    /// The offer as it read the last time it was seen.
    offer: Offer,
    /// The last price seen, in dollars per hour: a look that gave none does not erase it.
    price: Option<f64>,
    /// The index of the look where this run of sightings began.
    first: usize,
    /// The index of the look where it was last seen.
    last: usize,
    /// In how many looks, since `first`, it was seen.
    count: usize,
}

/// A price as the table shows it: to the thousandth of a dollar.
fn shown(price: f64) -> String {
    format!("{price:.3}")
}

/// The cheaper offer first, then the lower id; an offer with no price last.
fn by_price(a: &Offer, b: &Offer) -> std::cmp::Ordering {
    a.dph_total
        .unwrap_or(f64::INFINITY)
        .total_cmp(&b.dph_total.unwrap_or(f64::INFINITY))
        .then(a.id.cmp(&b.id))
}

/// The machines of a watch, over its looks.
///
/// ```
/// use vast_skopos::{market::MarketWatch, offer::Offer};
/// let at = |machine: u64, price: f64| Offer::from_json(&serde_json::json!(
///   {"id": machine * 10, "machine_id": machine, "num_gpus": 1, "dph_total": price})).unwrap();
/// let mut watch = MarketWatch::default();
/// for _ in 0..3 {
///     assert!(watch.record(&[at(1, 0.5), at(2, 0.6)]).is_none(), "still learning");
/// }
/// let report = watch.record(&[at(1, 0.5), at(2, 0.6), at(3, 0.7)]).unwrap();
/// assert_eq!(report.appeared.len(), 1);
/// assert_eq!(watch.seen(&at(1, 0.5)), Some((4, 4)), "seen in all four looks");
/// assert_eq!(watch.seen(&at(3, 0.7)), Some((1, 1)));
/// ```
#[derive(Debug, Clone, Default)]
pub struct MarketWatch {
    /// How many looks were recorded.
    looks: usize,
    /// The machines currently remembered.
    entries: HashMap<OfferKey, Entry>,
}

impl MarketWatch {
    /// How many looks were recorded.
    #[must_use]
    pub const fn looks(&self) -> usize {
        self.looks
    }

    /// Records one look: the offers of an answer, in the order the caller shows them.
    ///
    /// Returns what moved since the machines were last seen, or `None` while the watch is still
    /// learning (its first [`LEARNING_LOOKS`] looks). Two offers of one machine with the same GPU
    /// count count once, the cheaper (then the lower id), whatever order the caller sorted them in:
    /// the one kept does not flip from look to look and does not read as a price that moved.
    pub fn record(&mut self, offers: &[Offer]) -> Option<OfferChanges> {
        let at = self.looks;
        self.looks += 1;
        let reporting = at >= LEARNING_LOOKS;
        let mut report = OfferChanges::default();
        // One offer per machine in this look: the cheapest, in the order the machines first appear.
        let mut order: Vec<OfferKey> = Vec::new();
        let mut best: HashMap<OfferKey, &Offer> = HashMap::new();
        for offer in offers {
            let key = OfferKey::of(offer);
            match best.get(&key) {
                None => {
                    order.push(key);
                    best.insert(key, offer);
                }
                Some(kept) if by_price(offer, kept).is_lt() => {
                    best.insert(key, offer);
                }
                Some(_) => {}
            }
        }
        let in_look: HashSet<OfferKey> = order.iter().copied().collect();
        for key in order {
            let Some(offer) = best.get(&key).copied() else {
                continue;
            };
            match self.entries.get_mut(&key) {
                None => {
                    self.entries.insert(
                        key,
                        Entry {
                            // BORROW: explicit clone -- the watch keeps its own copy of the offer
                            offer: offer.clone(),
                            price: offer.dph_total,
                            first: at,
                            last: at,
                            count: 1,
                        },
                    );
                    if reporting {
                        // BORROW: explicit clone -- the report owns its offers
                        report.appeared.push(offer.clone());
                    }
                }
                Some(entry) => {
                    let missed = at - entry.last - 1;
                    if missed >= ABSENT_FOR_NEW {
                        // Back after a long absence: new again, and its sightings start afresh.
                        if reporting {
                            // BORROW: explicit clone -- the report owns its offers
                            report.appeared.push(offer.clone());
                        }
                        entry.first = at;
                        entry.count = 0;
                    } else if reporting
                        && let (Some(was), Some(now)) = (entry.price, offer.dph_total)
                        && shown(was) != shown(now)
                    {
                        report.repriced.push(Repriced {
                            // BORROW: explicit clone -- the report owns its offers
                            offer: offer.clone(),
                            was,
                        });
                    }
                    // BORROW: explicit clone -- the watch keeps its own copy of the offer
                    entry.offer = offer.clone();
                    entry.price = offer.dph_total.or(entry.price);
                    entry.last = at;
                    entry.count += 1;
                }
            }
        }
        let leaving: Vec<OfferKey> = self
            .entries
            .iter()
            .filter(|(key, entry)| !in_look.contains(key) && at - entry.last >= ABSENT_FOR_GONE)
            .map(|(key, _)| *key)
            .collect();
        for key in leaving {
            if let Some(entry) = self.entries.remove(&key)
                && reporting
            {
                report.gone.push(entry.offer);
            }
        }
        report.gone.sort_by(by_price);
        reporting.then_some(report)
    }

    /// In how many looks `offer`'s machine appeared, out of how many since it was first seen:
    /// `(3, 9)` is three looks of the nine since its first. `None` for a machine the watch does
    /// not remember.
    #[must_use]
    pub fn seen(&self, offer: &Offer) -> Option<(usize, usize)> {
        self.entries
            .get(&OfferKey::of(offer))
            .map(|entry| (entry.count, self.looks - entry.first))
    }
}

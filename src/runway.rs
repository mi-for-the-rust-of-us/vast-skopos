//! The account's runway: its credit, what the boxes burn per hour, and how long that lasts.
//!
//! The money line of `vas` and the `money` object of `vas --compact` are both read from one
//! [`Runway`], so they cannot disagree.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::instance::{Account, Instance};

/// `x` with `decimals` decimals; a value that rounds to zero loses its sign (`$-0.00` is nonsense).
fn dollars(x: f64, decimals: usize) -> String {
    let text = format!("{x:.decimals$}");
    match text.strip_prefix('-') {
        Some(rest) if rest.bytes().all(|b| b == b'0' || b == b'.') => {
            // BORROW: explicit to_owned -- the cell is owned by the line
            rest.to_owned()
        }
        _ => text,
    }
}

/// The account's runway over the fleet: the figures the money line prints and the compact output
/// carries.
///
/// The burn counts the instances the API reads as `running` AND those it was asked to run
/// (`intended_status`) that read otherwise (`offline`, `loading`, ...): whether those bill is not
/// known from here, and an under-counted burn overstates the runway.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Runway {
    /// The prepaid credit, in dollars, when the account answered.
    pub credit: Option<f64>,
    /// The burn, in dollars per hour: the running instances' `$/h` and the unsure ones'.
    pub burn: f64,
    /// How many instances read `running`.
    pub running: usize,
    /// The instances meant to run that read otherwise, counted by their state (`unknown` when the
    /// API gave none).
    pub unsure: BTreeMap<String, usize>,
}

impl Runway {
    /// The money of `instances` under `account`.
    ///
    /// ```
    /// use vast_skopos::{instance::{Account, Instance}, runway::Runway};
    /// let row = |id, actual: &str| Instance::from_json(&serde_json::json!(
    ///   {"id": id, "actual_status": actual, "intended_status": "running", "dph_total": 0.5})).unwrap();
    /// let a = Account::from_json(&serde_json::json!({"credit": 5.0}));
    /// let m = Runway::of(&[row(1, "running"), row(2, "offline")], Some(&a));
    /// assert_eq!((m.running, m.burn, m.unsure.get("offline").copied()), (1, 1.0, Some(1)));
    /// assert_eq!(m.hours_left(), Some(5.0));
    /// ```
    #[must_use]
    pub fn of(instances: &[Instance], account: Option<&Account>) -> Self {
        let running: Vec<&Instance> = instances.iter().filter(|i| i.is_running()).collect();
        let unsure: Vec<&Instance> = instances
            .iter()
            .filter(|i| i.is_meant_to_run() && !i.is_running())
            .collect();
        // A fold from +0.0, not `sum()`: an empty f64 sum is -0.0, which would print as `$-0.000`.
        let burn = running
            .iter()
            .chain(&unsure)
            .filter_map(|i| i.dph_total)
            .fold(0.0_f64, |acc, d| acc + d);
        let mut by_state: BTreeMap<String, usize> = BTreeMap::new();
        for i in &unsure {
            *by_state
                .entry(
                    i.actual_status
                        .as_deref()
                        .unwrap_or("unknown")
                        // BORROW: explicit to_owned -- the map owns its keys
                        .to_owned(),
                )
                .or_default() += 1;
        }
        Self {
            credit: account.and_then(|a| a.credit),
            burn,
            running: running.len(),
            unsure: by_state,
        }
    }

    /// The hours the credit lasts at this burn; `None` without a credit (or with none left: a
    /// debt has no runway), without a burn, or when the burn is so small that the hours are not a
    /// number worth showing.
    #[must_use]
    pub fn hours_left(&self) -> Option<f64> {
        self.credit
            .filter(|c| *c > 0.0)
            .filter(|_| self.burn > 0.0)
            .map(|c| c / self.burn)
            .filter(|h| h.is_finite())
    }

    /// The money line, ending in a newline (see [`crate::render::money_line`]).
    #[must_use]
    pub fn line(&self) -> String {
        let mut s = format!(
            "credit {} | burn ${}/h over {} running",
            self.credit
                .map_or_else(|| String::from("?"), |c| format!("${}", dollars(c, 2))),
            dollars(self.burn, 3),
            self.running
        );
        if !self.unsure.is_empty() {
            let states: Vec<String> = self
                .unsure
                .iter()
                .map(|(state, n)| format!("{n} {state}"))
                .collect();
            let _ = write!(
                s,
                " + {} (meant to run, billing unknown)",
                states.join(", ")
            );
        }
        if let Some(h) = self.hours_left() {
            let _ = write!(s, " | ~{h:.1} h left at this rate");
        }
        s.push('\n');
        s
    }
}

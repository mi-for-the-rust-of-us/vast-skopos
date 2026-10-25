//! `vas --until PATTERN`: whether a look found the line a watch waits for, or whether there is
//! still something to wait on.
//!
//! Under `--until`, every box is asked for the last line of its log matching the pattern
//! ([`crate::probe::LogQuery::grep`]), so a [`LogReading::Line`] IS a match. The binary counts the
//! looks with nothing to wait on and gives up after [`UNREAD_LOOKS`] of them in a row: the rule of
//! the hand-written loops `--until` replaces ("stop after two `ssh` failures"), kept in one place.

use crate::instance::Instance;
use crate::probe::{LogReading, ProbeOutcome, outcome_at};

/// The looks in a row with nothing to wait on ([`Verdict::Unread`]) after which a wait gives up.
pub const UNREAD_LOOKS: u32 = 2;

/// What one look says to a wait.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Verdict {
    /// A box's log has a line matching: the first such box, in the instances' order.
    Matched {
        /// The instance's id.
        id: u64,
        /// The matching line, as the table shows it (cleaned, at most 120 characters).
        line: String,
    },
    /// No match yet, and something to wait on: a box was read (its log does not match yet, or no
    /// file matches its path yet), or a box meant to run is still starting.
    Waiting,
    /// Nothing to wait on: every probe failed, or no box is running or starting.
    Unread,
}

/// What the look of `instances` and their `probes` (one outcome per instance, in the instances'
/// order, as [`crate::probe::probe_all`] gives them) says to a wait.
///
/// A box meant to run that is not running yet (`loading`, say) keeps the wait going: a box rented
/// a minute ago must not count against it.
///
/// ```
/// use vast_skopos::{instance::Instance, probe::parse_output, until::{Verdict, verdict}};
/// let running = Instance::from_json(&serde_json::json!({"id": 7, "actual_status": "running"})).unwrap();
/// let look = |log: &str| Some(Ok(parse_output(&format!("0, 99, 1, 2\n---vas-log---\n{log}"))));
/// let found = look("vas-grep-file:/r/run.log\ninitial loss 4.21\nvas-grep-exit:0\n");
/// assert_eq!(verdict(std::slice::from_ref(&running), &[found]),
///     Verdict::Matched { id: 7, line: "initial loss 4.21".to_owned() });
/// let not_yet = look("vas-grep-file:/r/run.log\nvas-grep-exit:1\n");
/// assert_eq!(verdict(std::slice::from_ref(&running), &[not_yet]), Verdict::Waiting);
/// assert_eq!(verdict(&[running], &[None]), Verdict::Unread);
/// ```
#[must_use]
pub fn verdict(instances: &[Instance], probes: &[ProbeOutcome]) -> Verdict {
    let mut read = false;
    // EXPLICIT: an imperative loop: the first match ends the search, and every read box counts.
    for (n, i) in instances.iter().enumerate() {
        let Some(Ok(probe)) = outcome_at(probes, n) else {
            continue;
        };
        if let Some(LogReading::Line { line, .. }) = &probe.log {
            return Verdict::Matched {
                id: i.id,
                // BORROW: explicit clone -- the verdict outlives the look's probes
                line: line.clone(),
            };
        }
        read = true;
    }
    let starting = instances
        .iter()
        .any(|i| i.is_meant_to_run() && !i.is_running());
    if read || starting {
        Verdict::Waiting
    } else {
        Verdict::Unread
    }
}

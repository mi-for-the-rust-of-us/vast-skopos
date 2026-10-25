//! `vas --until PATTERN`: whether a look found the line a watch waits for, or whether there is
//! still something to wait on, and when a wait ends.
//!
//! Under `--until`, every box is asked for the last line of its log matching the pattern
//! ([`crate::probe::LogQuery::grep`]), so a [`LogReading::Line`] IS a match. [`verdict`] reads one
//! look; [`Wait`] keeps the count across looks and says when the wait is over: on a match, on a
//! pattern the box's `grep` refuses, or after [`UNREAD_LOOKS`] looks in a row with nothing to wait
//! on (the rule of the hand-written loops `--until` replaces, "stop after two `ssh` failures", kept
//! in one place).

use crate::instance::Instance;
use crate::probe::{LogReading, ProbeOutcome, outcome_at};

/// The looks in a row with nothing to wait on after which a wait gives up.
pub const UNREAD_LOOKS: u32 = 2;

/// The looks a box meant to run but not running may keep a wait going, counted since a box was last
/// read.
///
/// After them, such a look counts as one with nothing to wait on. A box rented a minute ago is
/// `loading` for a few looks; one whose host went `offline` must not hold a wait forever. A look
/// with nothing to read in between does not start the count afresh: a box flipping between
/// `loading` and unreachable must not hold a wait forever either.
pub const STARTING_LOOKS: u32 = 10;

/// What one look says to a wait.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Verdict {
    /// A box's log has a line matching: the first such box, in the instances' order.
    Matched {
        /// The instance's id.
        id: u64,
        /// The matching line, as the table shows it (cleaned, cut at 120 characters).
        line: String,
    },
    /// No match, a box's `grep` refused the pattern (one it cannot read as an extended regular
    /// expression, or a `grep` that lacks the options the probe gives it), and no other box was read
    /// that could still match.
    Refused {
        /// The instance's id: the first such box, in the instances' order.
        id: u64,
    },
    /// No match yet, and a box was read: its log does not match yet, or no file matches its path
    /// yet. A log the box could not give ([`LogReading::Unreadable`]: the search ran past its
    /// deadline, the file could not be read, the answer was cut short) is NOT a box read.
    Read,
    /// No box was read, but a box meant to run is not running (yet): `loading`, say.
    Starting,
    /// Nothing to wait on: no box was read and none is meant to run but not running. Every probe
    /// failed or gave no readable log, or none could be made (no box running, or none with an `ssh`
    /// endpoint).
    Unread,
}

/// What the look of `instances` and their `probes` (one outcome per instance, in the instances'
/// order, as [`crate::probe::probe_all`] gives them) says to a wait.
///
/// ```
/// use vast_skopos::{instance::Instance, probe::parse_output, until::{Verdict, verdict}};
/// let running = Instance::from_json(&serde_json::json!({"id": 7, "actual_status": "running"})).unwrap();
/// let look = |log: &str| Some(Ok(parse_output(&format!("0, 99, 1, 2\n---vas-log---\n{log}"))));
/// let found = look("vas-grep-file:/r/run.log\ninitial loss 4.21\nvas-grep-exit:0\n");
/// assert_eq!(verdict(std::slice::from_ref(&running), &[found]),
///     Verdict::Matched { id: 7, line: "initial loss 4.21".to_owned() });
/// let not_yet = look("vas-grep-file:/r/run.log\nvas-grep-exit:1\n");
/// assert_eq!(verdict(std::slice::from_ref(&running), &[not_yet]), Verdict::Read);
/// let refused = look("vas-grep-file:/r/run.log\nvas-grep-exit:2\n");
/// assert_eq!(verdict(std::slice::from_ref(&running), &[refused]), Verdict::Refused { id: 7 });
/// assert_eq!(verdict(&[running], &[None]), Verdict::Unread);
/// ```
#[must_use]
pub fn verdict(instances: &[Instance], probes: &[ProbeOutcome]) -> Verdict {
    let (mut read, mut refused) = (false, None);
    // EXPLICIT: an imperative loop: the first match ends the search, and every read box counts.
    for (n, i) in instances.iter().enumerate() {
        let Some(Ok(probe)) = outcome_at(probes, n) else {
            continue;
        };
        match &probe.log {
            Some(LogReading::Line { line, .. }) => {
                return Verdict::Matched {
                    id: i.id,
                    // BORROW: explicit clone -- the verdict outlives the look's probes
                    line: line.clone(),
                };
            }
            Some(LogReading::PatternRefused { .. }) => refused = refused.or(Some(i.id)),
            // EXPLICIT: a log the box could not give is no box read: it must not hold a wait.
            Some(LogReading::Unreadable) => {}
            Some(
                LogReading::NoMatch
                | LogReading::NotAFile { .. }
                | LogReading::Empty { .. }
                | LogReading::NoMatchingLine { .. },
            )
            | None => read = true,
        }
    }
    // A box that took the pattern can still match: one box's refusal ends a wait only alone.
    if read {
        Verdict::Read
    } else if let Some(id) = refused {
        Verdict::Refused { id }
    } else if instances
        .iter()
        .any(|i| i.is_meant_to_run() && !i.is_running())
    {
        Verdict::Starting
    } else {
        Verdict::Unread
    }
}

/// How a wait ended.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum End {
    /// A box's log has a matching line ([`Verdict::Matched`]).
    Matched {
        /// The instance's id.
        id: u64,
        /// The matching line.
        line: String,
    },
    /// A box's `grep` refused the pattern ([`Verdict::Refused`]).
    Refused {
        /// The instance's id.
        id: u64,
    },
    /// No box could be read for [`UNREAD_LOOKS`] looks in a row.
    GaveUp,
}

/// A wait across looks: the looks in a row with nothing to wait on, and those since a box was last
/// read in which only a box that is not running kept it going.
///
/// ```
/// use vast_skopos::until::{End, UNREAD_LOOKS, Verdict, Wait};
/// let mut wait = Wait::default();
/// assert_eq!(wait.record(Verdict::Unread), None);
/// assert_eq!(wait.record(Verdict::Read), None, "a box read starts the count afresh");
/// assert_eq!(wait.record(Verdict::Unread), None);
/// assert_eq!(wait.record(Verdict::Unread), Some(End::GaveUp), "{UNREAD_LOOKS} in a row");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Wait {
    /// The looks in a row with nothing to wait on.
    unread: u32,
    /// The looks since a box was last read in which a box meant to run but not running kept the
    /// wait going.
    starting: u32,
}

impl Wait {
    /// Counts one look's `verdict`; the end of the wait, when this look ends it.
    ///
    /// A [`Verdict::Read`] starts both counts afresh; a [`Verdict::Starting`] keeps the wait going
    /// for [`STARTING_LOOKS`] looks since a box was last read, then counts as [`Verdict::Unread`].
    ///
    /// ```
    /// use vast_skopos::until::{End, STARTING_LOOKS, Verdict, Wait};
    /// let mut wait = Wait::default();
    /// for _ in 0..STARTING_LOOKS {
    ///     assert_eq!(wait.record(Verdict::Starting), None);
    /// }
    /// assert_eq!(wait.record(Verdict::Starting), None, "one look with nothing to wait on");
    /// assert_eq!(wait.record(Verdict::Starting), Some(End::GaveUp), "and a second");
    /// assert_eq!(Wait::default().record(Verdict::Refused { id: 7 }), Some(End::Refused { id: 7 }));
    /// ```
    #[must_use]
    pub fn record(&mut self, verdict: Verdict) -> Option<End> {
        let nothing_to_wait_on = match verdict {
            Verdict::Matched { id, line } => return Some(End::Matched { id, line }),
            Verdict::Refused { id } => return Some(End::Refused { id }),
            Verdict::Read => {
                *self = Self::default();
                return None;
            }
            Verdict::Starting => {
                self.starting = self.starting.saturating_add(1);
                self.starting > STARTING_LOOKS
            }
            Verdict::Unread => true,
        };
        if nothing_to_wait_on {
            self.unread = self.unread.saturating_add(1);
        } else {
            self.unread = 0;
        }
        (self.unread >= UNREAD_LOOKS).then_some(End::GaveUp)
    }
}

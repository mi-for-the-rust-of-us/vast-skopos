//! `vas --until PATTERN`: whether a look found the line a watch waits for, or whether there is
//! still something to wait on, and when a wait ends.
//!
//! Under `--until`, every box is asked for the last line of its log matching the pattern
//! ([`crate::probe::LogQuery::grep`]), so a [`LogReading::Line`] IS a match. [`verdict`] reads one
//! look; [`Wait`] keeps the count across looks and says when the wait is over: on a match, on a
//! pattern the box's `grep` refuses, after [`UNREAD_LOOKS`] looks in a row with nothing to wait on
//! (the rule of the hand-written loops `--until` replaces, "stop after two `ssh` failures", kept in
//! one place), or when no box has had a file matching the path for [`NO_FILE_SECS`] seconds, or
//! the time set with [`Wait::no_file_secs`] (a path that is wrong, or that the shell rewrote, must
//! not hold a wait forever).

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

/// The looks in a row with no box holding a file that matches the `--log` path after which a wait
/// warns that the path may be wrong ([`Wait::warns_of_no_file`]).
pub const NO_FILE_WARN_LOOKS: u32 = 2;

/// The seconds (15 minutes) a wait goes on, by default, while no box holds a file that matches the
/// `--log` path, counted from the first look that found none; then it gives up ([`End::NoFile`]).
///
/// A run may write its log late (a setup or a speed check first), and [`Wait::no_file_secs`] sets
/// a longer time; but a path that matches nothing on any box (seen 2026-10-26: Git Bash had
/// rewritten it) must not hold a wait forever, and a script that waits on it pays for the boxes
/// meanwhile.
pub const NO_FILE_SECS: u64 = 900;

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
    /// No match yet, and a box was read that holds a file matching the path: its log does not match
    /// yet. A log the box could not give ([`LogReading::Unreadable`]: the search ran past its
    /// deadline, the file could not be read, the answer was cut short) is NOT a box read.
    Read,
    /// Boxes were read, but none holds a file matching the path (each read box says "no file
    /// matches", or that the newest match is not a file), no box's log was unreadable (it may hold
    /// the file), and every box meant to run is running: the log is not written yet, or the path is
    /// wrong.
    NoFile,
    /// No box was read with a file to search, but a box meant to run is not running (yet):
    /// `loading`, say. Its log may come once it runs.
    Starting,
    /// Nothing to wait on: no box was read with a file to search, none is meant to run but not
    /// running, and not every box read can be said to lack the file. Every probe failed or gave no
    /// readable log, or none could be made (no box running, or none with an `ssh` endpoint).
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
/// assert_eq!(verdict(std::slice::from_ref(&running), &[look("vas-no-file\n")]), Verdict::NoFile);
/// assert_eq!(verdict(&[running], &[None]), Verdict::Unread);
/// ```
#[must_use]
pub fn verdict(instances: &[Instance], probes: &[ProbeOutcome]) -> Verdict {
    let (mut read, mut no_file, mut unreadable, mut refused) = (false, false, false, None);
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
            // A log the box could not give is no box read, and it may hold the file: it says
            // nothing either way.
            Some(LogReading::Unreadable) => unreadable = true,
            // Read, but without a file to search: the path may be wrong.
            Some(LogReading::NoMatch | LogReading::NotAFile { .. }) => no_file = true,
            // Without `--log` there is no path to doubt: a box read is a box read.
            Some(LogReading::Empty { .. } | LogReading::NoMatchingLine { .. }) | None => {
                read = true;
            }
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
        // A box still starting may write the file once it runs.
        Verdict::Starting
    } else if no_file && !unreadable {
        Verdict::NoFile
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
    /// No box held a file matching the path for [`NO_FILE_SECS`] seconds (or the time set with
    /// [`Wait::no_file_secs`]).
    NoFile,
}

/// A wait across looks, and what it has counted.
///
/// It keeps the looks in a row with nothing to wait on, those since a box was last read in which
/// only a box that is not running kept it going, and since when no box has held a file matching
/// the path.
///
/// ```
/// use vast_skopos::until::{End, UNREAD_LOOKS, Verdict, Wait};
/// let mut wait = Wait::default();
/// assert_eq!(wait.record(Verdict::Unread), None);
/// assert_eq!(wait.record(Verdict::Read), None, "a box read starts the count afresh");
/// assert_eq!(wait.record(Verdict::Unread), None);
/// assert_eq!(wait.record(Verdict::Unread), Some(End::GaveUp), "{UNREAD_LOOKS} in a row");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Wait {
    /// The looks in a row with nothing to wait on.
    unread: u32,
    /// The looks since a box was last read in which a box meant to run but not running kept the
    /// wait going.
    starting: u32,
    /// The looks since a box was last read with the file in which boxes were read but none held a
    /// file matching the path.
    no_file: u32,
    /// The Unix second of the first of those looks; `None` since a box was last read with the file.
    no_file_since: Option<u64>,
    /// The seconds a wait goes on while no box holds the file.
    no_file_secs: u64,
    /// Whether the look just recorded is the one to warn on ([`Wait::warns_of_no_file`]).
    warn: bool,
}

impl Default for Wait {
    /// A wait that gives up on a missing file after [`NO_FILE_SECS`].
    fn default() -> Self {
        Self {
            unread: 0,
            starting: 0,
            no_file: 0,
            no_file_since: None,
            no_file_secs: NO_FILE_SECS,
            warn: false,
        }
    }
}

impl Wait {
    /// The same wait, giving up after `secs` seconds (instead of [`NO_FILE_SECS`]) in which no box
    /// has held a file matching the path.
    ///
    /// ```
    /// use vast_skopos::until::{End, Verdict, Wait};
    /// let mut wait = Wait::default().no_file_secs(60);
    /// assert_eq!(wait.record_at(Verdict::NoFile, 1_000), None, "the first look without the file");
    /// assert_eq!(wait.record_at(Verdict::NoFile, 1_059), None);
    /// assert_eq!(wait.record_at(Verdict::NoFile, 1_060), Some(End::NoFile));
    /// ```
    #[must_use]
    pub const fn no_file_secs(mut self, secs: u64) -> Self {
        self.no_file_secs = secs;
        self
    }

    /// Counts one look's `verdict`, the look taken now (this machine's clock); the end of the wait,
    /// when this look ends it. [`Wait::record_at`] says how.
    ///
    /// ```
    /// use vast_skopos::until::{End, Verdict, Wait};
    /// let mut wait = Wait::default();
    /// assert_eq!(wait.record(Verdict::Read), None);
    /// assert_eq!(wait.record(Verdict::Refused { id: 7 }), Some(End::Refused { id: 7 }));
    /// ```
    #[must_use]
    pub fn record(&mut self, verdict: Verdict) -> Option<End> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.record_at(verdict, now)
    }

    /// Counts one look's `verdict`, the look taken at Unix second `at`; the end of the wait, when
    /// this look ends it.
    ///
    /// A [`Verdict::Read`] starts every count afresh; a [`Verdict::Starting`] keeps the wait going
    /// for [`STARTING_LOOKS`] looks since a box was last read, then counts as [`Verdict::Unread`].
    /// A [`Verdict::NoFile`] ends the wait once `at` is [`NO_FILE_SECS`] (or the time set with
    /// [`Wait::no_file_secs`]) past the first such look since a box was last read with the file;
    /// the looks between with nothing to read do not start that time afresh. A clock that goes
    /// back ends nothing.
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
    pub fn record_at(&mut self, verdict: Verdict, at: u64) -> Option<End> {
        self.warn = false;
        let nothing_to_wait_on = match verdict {
            Verdict::Matched { id, line } => return Some(End::Matched { id, line }),
            Verdict::Refused { id } => return Some(End::Refused { id }),
            Verdict::Read => {
                *self = Self::default().no_file_secs(self.no_file_secs);
                return None;
            }
            // A box was read: ssh works; the path is what may be wrong.
            Verdict::NoFile => {
                self.unread = 0;
                self.no_file = self.no_file.saturating_add(1);
                self.warn = self.no_file == NO_FILE_WARN_LOOKS;
                let since = *self.no_file_since.get_or_insert(at);
                return (at.saturating_sub(since) >= self.no_file_secs).then_some(End::NoFile);
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

    /// Whether the look just recorded is the [`NO_FILE_WARN_LOOKS`]-th since a box was last read
    /// with the file in which boxes were read but none held a file matching the path: the one look
    /// to warn on that the path may be wrong. The looks with nothing to read do not count, nor
    /// warn again.
    ///
    /// ```
    /// use vast_skopos::until::{NO_FILE_WARN_LOOKS, Verdict, Wait};
    /// let mut wait = Wait::default();
    /// let warned: Vec<bool> = [Verdict::NoFile, Verdict::NoFile, Verdict::Unread, Verdict::NoFile]
    ///     .into_iter()
    ///     .map(|v| {
    ///         let _ = wait.record_at(v, 0);
    ///         wait.warns_of_no_file()
    ///     })
    ///     .collect();
    /// assert_eq!(NO_FILE_WARN_LOOKS, 2);
    /// assert_eq!(warned, [false, true, false, false], "once");
    /// ```
    #[must_use]
    pub const fn warns_of_no_file(&self) -> bool {
        self.warn
    }
}

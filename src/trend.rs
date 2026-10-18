//! What the GPUs of each box did over the last few looks of a watch.
//!
//! One `nvidia-smi` sample is not a diagnosis (it has read 100 % with 0 MiB used while nothing held
//! the card, and the dashboard has read 0 % on boxes at 97-99 %): a watch keeps the last
//! [`TREND_LEN`] samples of each box, oldest first, so a run that stalls reads `99 99 98 0 0`, and a
//! lone odd reading reads `99 99 0 99 99`.
//!
//! The history lives in memory, for the length of the watch; nothing is written anywhere.

use std::collections::{HashMap, VecDeque};

use crate::instance::Instance;
use crate::probe::{ProbeOutcome, outcome_at};

/// How many samples a box keeps.
pub const TREND_LEN: usize = 6;

/// The last utilisation samples of each box, by instance id.
///
/// A sample is the mean utilisation of the box's GPUs, in percent, rounded to the nearest whole
/// number.
///
/// ```
/// use vast_skopos::{instance::Instance, probe::parse_output, trend::Trend};
/// let box7 = Instance::from_json(&serde_json::json!({"id": 7, "actual_status": "running"})).unwrap();
/// let mut trend = Trend::default();
/// for util in [99, 98, 0] {
///     let probe = Some(Ok(parse_output(&format!("0, {util}, 100, 200\n"))));
///     trend.record(std::slice::from_ref(&box7), &[probe]);
/// }
/// assert_eq!(trend.samples(7), [99, 98, 0]);
/// assert_eq!(trend.text(7), "99 98 0");
/// assert_eq!(trend.text(8), "-", "a box with no sample");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Trend {
    /// The samples of each box, oldest first, at most [`TREND_LEN`].
    by_box: HashMap<u64, VecDeque<u32>>,
}

/// The mean of the GPUs' utilisation, rounded; `None` without a GPU reading.
fn mean_util(utils: &[u32]) -> Option<u32> {
    let count = u32::try_from(utils.len()).ok().filter(|n| *n > 0)?;
    let sum: u32 = utils.iter().sum();
    Some((sum + count / 2) / count)
}

impl Trend {
    /// Records one look: the mean GPU utilisation of every box that was probed and answered.
    ///
    /// A probe that failed adds nothing and keeps what the box had (one dropped `ssh` is not the
    /// run stopping). A box that is no longer in `instances`, or no longer running, loses its
    /// history: the numbers of its last run say nothing of the next.
    pub fn record(&mut self, instances: &[Instance], probes: &[ProbeOutcome]) {
        self.by_box
            .retain(|id, _| instances.iter().any(|i| i.id == *id && i.is_running()));
        for (n, instance) in instances.iter().enumerate() {
            let Some(Ok(probe)) = outcome_at(probes, n) else {
                continue;
            };
            let utils: Vec<u32> = probe.gpus.iter().map(|g| g.util_pct).collect();
            let Some(sample) = mean_util(&utils) else {
                continue;
            };
            let history = self.by_box.entry(instance.id).or_default();
            if history.len() == TREND_LEN {
                history.pop_front();
            }
            history.push_back(sample);
        }
    }

    /// The samples of box `id`, oldest first; empty when it has none.
    #[must_use]
    pub fn samples(&self, id: u64) -> Vec<u32> {
        self.by_box
            .get(&id)
            .map(|history| history.iter().copied().collect())
            .unwrap_or_default()
    }

    /// The samples of box `id` as the table shows them (`97 99 0 98`), or `-` without any.
    #[must_use]
    pub fn text(&self, id: u64) -> String {
        let samples = self.samples(id);
        if samples.is_empty() {
            String::from("-")
        } else {
            samples
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        }
    }
}

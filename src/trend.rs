//! What the GPUs of each box did over the last few looks of a watch.
//!
//! One `nvidia-smi` sample is not a diagnosis (it has read 100 % with 0 MiB used while nothing held
//! the card, and the dashboard has read 0 % on boxes at 97-99 %): a watch keeps the last
//! [`TREND_LEN`] samples of each box, oldest first, so a run that stalls reads `99 99 98 0 0`, and a
//! lone odd reading reads `99 99 0 99 99`.
//!
//! The same history notices when a GPU's power limit or its clock throttle reasons change between
//! two looks (a host lowering the cap mid-run, a card starting to run hot), or when another user of
//! the GPU appears or leaves (memory no process of the box accounts for): [`GpuChange`], which the
//! table and `--compact` print under the look.
//!
//! The history lives in memory, for the length of the watch; nothing is written anywhere.

use std::collections::{HashMap, VecDeque};

use crate::instance::Instance;
use crate::probe::{GpuReading, OTHER_MIB_MIN, ProbeOutcome, outcome_at};

/// How many samples a box keeps.
pub const TREND_LEN: usize = 6;

/// What changed on one GPU of a box between two looks: its power limit, what holds its clocks
/// down, or whether processes outside the container hold its memory.
///
/// ```
/// use vast_skopos::{instance::Instance, probe::parse_output, trend::Trend};
/// let box7 = Instance::from_json(&serde_json::json!({"id": 7, "actual_status": "running"})).unwrap();
/// let look = |limit: u32, mask: &str| Some(Ok(parse_output(&format!(
///   "0, 100, 1, 2\n---vas-power---\n0, 399, {limit}, 575, 1700, 3090, {mask}\n"))));
/// let mut trend = Trend::default();
/// trend.record(std::slice::from_ref(&box7), &[look(575, "0x0")]);
/// assert!(trend.changes().is_empty(), "the first look is the baseline");
/// trend.record(std::slice::from_ref(&box7), &[look(400, "0x4")]);
/// assert_eq!(trend.changes()[0].line(), "! 7 gpu 0: power limit 575 -> 400 W, power_cap began");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GpuChange {
    /// The instance's id.
    pub id: u64,
    /// The GPU's index on the box.
    pub gpu: u32,
    /// The power limit, in watts, before and after, when it moved.
    pub limit: Option<(u32, u32)>,
    /// The throttle reasons that began (see [`GpuReading::throttle_words`]).
    pub began: Vec<&'static str>,
    /// The throttle reasons that ended.
    pub ended: Vec<&'static str>,
    /// The memory no process of the box accounts for, in MiB, before and after, when it crossed
    /// [`OTHER_MIB_MIN`] one way or the other: someone else began (or stopped) using the GPU.
    pub other_mib: Option<(u64, u64)>,
}

impl GpuChange {
    /// The change as one line: `! 7 gpu 0: power limit 575 -> 400 W, power_cap began`.
    #[must_use]
    pub fn line(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some((was, now)) = self.limit {
            parts.push(format!("power limit {was} -> {now} W"));
        }
        if let Some((was, now)) = self.other_mib {
            parts.push(format!(
                "memory held outside the container {:.1} -> {:.1} GiB",
                gib(was),
                gib(now)
            ));
        }
        parts.extend(self.began.iter().map(|word| format!("{word} began")));
        parts.extend(self.ended.iter().map(|word| format!("{word} ended")));
        format!("! {} gpu {}: {}", self.id, self.gpu, parts.join(", "))
    }
}

/// What a watch last knew of one GPU's power: a reading the box did not give does not erase it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct PowerState {
    /// The power limit, in watts.
    limit: Option<u32>,
    /// The throttle reasons, as words.
    throttle: Option<Vec<&'static str>>,
    /// The memory no process of the box accounts for, in MiB.
    other_mib: Option<u64>,
}

/// MiB → GiB, for display.
fn gib(mib: u64) -> f64 {
    // CAST: u64 → f64, a MiB count far below 2^53, for a one-decimal display
    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    let v = mib as f64 / 1024.0;
    v
}

/// The last utilisation samples of each box, by instance id.
///
/// A sample is the mean utilisation of the box's GPUs, in percent, rounded to the nearest whole
/// number; the clock history keeps, the same way, the mean SM clock in MHz (a card whose clock
/// falls from 3000 to 1700 while it reads 100 % busy is being held back: see
/// [`GpuChange`] for why).
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
    /// The utilisation samples of each box, oldest first, at most [`TREND_LEN`].
    by_box: HashMap<u64, VecDeque<u32>>,
    /// The SM clock samples of each box (MHz, the mean of its GPUs), oldest first, at most
    /// [`TREND_LEN`].
    clocks: HashMap<u64, VecDeque<u32>>,
    /// The power of each GPU of each box, as last read.
    power: HashMap<u64, HashMap<u32, PowerState>>,
    /// What changed at the last [`record`](Trend::record).
    changes: Vec<GpuChange>,
}

/// The mean of the GPUs' utilisation, rounded; `None` without a GPU reading.
fn mean_util(utils: &[u32]) -> Option<u32> {
    let count = u32::try_from(utils.len()).ok().filter(|n| *n > 0)?;
    let sum: u32 = utils.iter().sum();
    Some((sum + count / 2) / count)
}

/// Compares `gpu` with what `known` holds of it and remembers the reading: the change, if the
/// power limit or the throttle reasons moved since the last look. A reading the box did not give
/// neither makes a change nor erases what was known.
fn note_power(
    known: &mut HashMap<u32, PowerState>,
    id: u64,
    gpu: &GpuReading,
) -> Option<GpuChange> {
    let now = PowerState {
        limit: gpu.power_limit_w,
        throttle: gpu.throttle_words(),
        other_mib: gpu.other_mem_mib,
    };
    let Some(was) = known.get_mut(&gpu.index) else {
        known.insert(gpu.index, now);
        return None;
    };
    let limit = match (was.limit, now.limit) {
        (Some(before), Some(after)) if before != after => Some((before, after)),
        _ => None,
    };
    let (began, ended) = match (&was.throttle, &now.throttle) {
        (Some(before), Some(after)) => (
            after
                .iter()
                .copied()
                .filter(|w| !before.contains(w))
                .collect(),
            before
                .iter()
                .copied()
                .filter(|w| !after.contains(w))
                .collect(),
        ),
        _ => (Vec::new(), Vec::new()),
    };
    let other = match (was.other_mib, now.other_mib) {
        (Some(before), Some(after)) if (before >= OTHER_MIB_MIN) != (after >= OTHER_MIB_MIN) => {
            Some((before, after))
        }
        _ => None,
    };
    was.other_mib = now.other_mib.or(was.other_mib);
    was.limit = now.limit.or(was.limit);
    was.throttle = now.throttle.or_else(|| was.throttle.take());
    (limit.is_some() || other.is_some() || !began.is_empty() || !ended.is_empty()).then_some(
        GpuChange {
            id,
            gpu: gpu.index,
            limit,
            began,
            ended,
            other_mib: other,
        },
    )
}

/// Adds `sample` to a history, dropping the oldest once it holds [`TREND_LEN`].
fn keep(history: &mut VecDeque<u32>, sample: u32) {
    if history.len() == TREND_LEN {
        history.pop_front();
    }
    history.push_back(sample);
}

/// Samples as the table shows them, separated by spaces, or `-` without any.
fn text_of(samples: &[u32]) -> String {
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

impl Trend {
    /// Records one look: the mean GPU utilisation and SM clock of every box that was probed and
    /// answered.
    ///
    /// A probe that failed adds nothing and keeps what the box had (one dropped `ssh` is not the
    /// run stopping). A box that is no longer in `instances`, or no longer running, loses its
    /// history: the numbers of its last run say nothing of the next.
    ///
    /// The power limit and the throttle reasons of each GPU are compared with the last look's:
    /// [`changes`](Trend::changes) then holds what moved (and nothing on a box's first look).
    pub fn record(&mut self, instances: &[Instance], probes: &[ProbeOutcome]) {
        let running = |id: &u64| instances.iter().any(|i| i.id == *id && i.is_running());
        self.by_box.retain(|id, _| running(id));
        self.clocks.retain(|id, _| running(id));
        self.power.retain(|id, _| running(id));
        self.changes.clear();
        for (n, instance) in instances.iter().enumerate() {
            let Some(Ok(probe)) = outcome_at(probes, n) else {
                continue;
            };
            let known = self.power.entry(instance.id).or_default();
            for gpu in &probe.gpus {
                if let Some(change) = note_power(known, instance.id, gpu) {
                    self.changes.push(change);
                }
            }
            let clocks: Vec<u32> = probe.gpus.iter().filter_map(|g| g.sm_mhz).collect();
            if let Some(sample) = mean_util(&clocks) {
                keep(self.clocks.entry(instance.id).or_default(), sample);
            }
            let utils: Vec<u32> = probe.gpus.iter().map(|g| g.util_pct).collect();
            if let Some(sample) = mean_util(&utils) {
                keep(self.by_box.entry(instance.id).or_default(), sample);
            }
        }
    }

    /// What changed in the power of the boxes at the last [`record`](Trend::record): empty on the
    /// first look, and when nothing moved.
    #[must_use]
    pub fn changes(&self) -> &[GpuChange] {
        &self.changes
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
        text_of(&self.samples(id))
    }

    /// The SM clock samples of box `id` in MHz, oldest first; empty when it has none.
    #[must_use]
    pub fn clock_samples(&self, id: u64) -> Vec<u32> {
        self.clocks
            .get(&id)
            .map(|history| history.iter().copied().collect())
            .unwrap_or_default()
    }

    /// The clock samples of box `id` as the table shows them (`3000 2900 1700`), or `-`.
    #[must_use]
    pub fn clock_text(&self, id: u64) -> String {
        text_of(&self.clock_samples(id))
    }
}

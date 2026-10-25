//! The same readings as the table, as JSON Lines: one object per box, then one for the money.
//!
//! For scripts and monitors (`vas --compact`): a line a program parses, where the table is a
//! picture. Every object has a `kind` (`box`, `event` or `money`) and `at` (Unix seconds, the
//! look's own clock, the same on every line of one look), so a line read alone says what it is and
//! when. The top-level keys of each `kind` are always the same: a value not known is `null`, never
//! a missing key. (`probe` has three shapes: `null` for a box not probed, `{"error": .., "error_kind": ..}`
//! for a probe that failed (`unreachable` when `ssh` never reached the box, else `failed`), else the
//! readings; `log` likewise has a `state` and what that state carries.)
//!
//! A look is the boxes, then one `event` per change of a GPU's power or other users (only under
//! `--ssh --watch`, see [`crate::trend::GpuChange`]), then the money, which closes the look.
//!
//! `vas --compact --watch N` prints one such look every N seconds, with no separator: the `at` of
//! each line tells the looks apart.

use serde_json::{Value, json};

use crate::error::Result;
use crate::instance::whole_secs;
use crate::instance::{Account, Instance};
use crate::probe::{LogReading, Probe, ProbeOutcome, outcome_at};
use crate::runway::Runway;
use crate::trend::{GpuChange, Trend};

/// `x` rounded to `decimals` places: a sum would print as `1.2860000000000003`, and vast.ai's own
/// credit arrives as `12.059765171599793`, float noise from its arithmetic.
fn rounded(x: f64, decimals: i32) -> f64 {
    let scale = 10_f64.powi(decimals);
    let r = (x * scale).round() / scale;
    // A value that rounds to zero is `0.0`, not `-0.0`: both are valid JSON, only one is sane.
    if r == 0.0 { 0.0 } else { r }
}

/// What `--log` found, as an object with a `state`; `null` when `--log` was not given.
fn log_json(log: Option<&LogReading>) -> Value {
    match log {
        None => Value::Null,
        Some(LogReading::NoMatch) => json!({"state": "no_match"}),
        Some(LogReading::NotAFile { file }) => json!({"state": "not_a_file", "file": file}),
        Some(LogReading::Empty { file }) => json!({"state": "empty", "file": file}),
        Some(LogReading::Line { file, line }) => {
            json!({"state": "line", "file": file, "line": line})
        }
        Some(LogReading::NoMatchingLine { file }) => {
            json!({"state": "no_matching_line", "file": file})
        }
        Some(LogReading::PatternRefused { file }) => {
            json!({"state": "pattern_refused", "file": file})
        }
        Some(LogReading::Unreadable) => json!({"state": "unreadable"}),
    }
}

/// One probe: its GPUs and its log, or the reason it failed; `null` when the box was not probed
/// (`--ssh` not given, or the box not running).
fn probe_json(
    probe: Option<&Result<Probe>>,
    trend: Option<&[u32]>,
    clock_trend: Option<&[u32]>,
) -> Value {
    match probe {
        None => Value::Null,
        Some(Err(e)) => json!({
            "error": e.to_string(),
            "error_kind": if e.is_unreachable() { "unreachable" } else { "failed" },
        }),
        Some(Ok(p)) => json!({
            "gpus": p.gpus.iter().map(|g| json!({
                "index": g.index,
                "util_pct": g.util_pct,
                "mem_used_mib": g.mem_used_mib,
                "mem_total_mib": g.mem_total_mib,
                "other_mem_mib": g.other_mem_mib,
                "power_w": g.power_w,
                "power_limit_w": g.power_limit_w,
                "power_default_w": g.power_default_w,
                "sm_mhz": g.sm_mhz,
                "sm_max_mhz": g.sm_max_mhz,
                "throttle": g.throttle_words(),
            })).collect::<Vec<_>>(),
            "log": log_json(p.log.as_ref()),
            "util_trend": trend,
            "clock_trend": clock_trend,
        }),
    }
}

/// One box's object.
fn box_json(i: &Instance, probe: Option<&Result<Probe>>, trend: Option<&Trend>, now: f64) -> Value {
    let endpoint = i.ssh_endpoint();
    let samples = trend.map(|t| t.samples(i.id));
    let clock_samples = trend.map(|t| t.clock_samples(i.id));
    json!({
        "kind": "box",
        "at": whole_secs(now),
        "id": i.id,
        "label": i.label,
        "gpu_name": i.gpu_name,
        "num_gpus": i.num_gpus,
        "where": i.geolocation,
        "status": i.actual_status,
        "intended": i.intended_status,
        "status_msg": i.status_msg,
        "dph": i.dph_total.map(|d| rounded(d, 4)),
        "age_s": i.age_secs(now).map(whole_secs),
        "cost": i.cost_so_far(now).map(|c| rounded(c, 4)),
        "ssh_host": endpoint.map(|(host, _)| host),
        "ssh_port": endpoint.map(|(_, port)| port),
        "machine_id": i.machine_id,
        "host_id": i.host_id,
        "cpu_name": i.cpu_name,
        "driver_version": i.driver_version,
        "dlperf": i.dlperf,
        "pcie_bw": i.pcie_bw,
        "probe": probe_json(probe, samples.as_deref(), clock_samples.as_deref()),
    })
}

/// One change of a box's power, as an object.
fn event_json(change: &GpuChange, now: f64) -> Value {
    json!({
        "kind": "event",
        "at": whole_secs(now),
        "id": change.id,
        "gpu": change.gpu,
        "limit_was": change.limit.map(|(was, _)| was),
        "limit_now": change.limit.map(|(_, now)| now),
        "other_mib_was": change.other_mib.map(|(was, _)| was),
        "other_mib_now": change.other_mib.map(|(_, now)| now),
        "began": change.began,
        "ended": change.ended,
    })
}

/// The money's object.
fn money_json(money: &Runway, now: f64) -> Value {
    json!({
        "kind": "money",
        "at": whole_secs(now),
        "credit": money.credit.map(|c| rounded(c, 4)),
        "burn": rounded(money.burn, 4),
        "running": money.running,
        "unsure": money.unsure,
        "hours_left": money.hours_left().map(|h| rounded(h, 1)),
    })
}

/// The look as JSON Lines: one object per instance, in the API's order, then the money's; each
/// line ends in a newline.
///
/// `probes` holds one outcome per instance, in the instances' order (see [`ProbeOutcome`]; a shorter
/// slice reads as "not probed" for the instances it does not reach); `now` is Unix seconds.
///
/// ```
/// use vast_skopos::{compact::json_lines, instance::{Account, Instance}};
/// let i = Instance::from_json(&serde_json::json!({"id": 7, "actual_status": "running",
///   "dph_total": 0.5, "start_date": 100.0, "machine_id": 9, "host_id": 5})).unwrap();
/// let a = Account::from_json(&serde_json::json!({"credit": 5.0}));
/// let text = json_lines(&[i], &[], None, Some(&a), 3700.0);
/// let lines: Vec<serde_json::Value> =
///     text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
/// assert_eq!((lines[0]["kind"].as_str(), lines[0]["id"].as_u64()), (Some("box"), Some(7)));
/// assert_eq!((lines[0]["machine_id"].as_u64(), lines[0]["host_id"].as_u64()), (Some(9), Some(5)));
/// assert_eq!((lines[0]["age_s"].as_u64(), lines[0]["cost"].as_f64()), (Some(3600), Some(0.5)));
/// assert!(lines[0]["label"].is_null() && lines[0]["probe"].is_null());
/// assert_eq!((lines[1]["kind"].as_str(), lines[1]["hours_left"].as_f64()), (Some("money"), Some(10.0)));
/// ```
#[must_use]
pub fn json_lines(
    instances: &[Instance],
    probes: &[ProbeOutcome],
    trend: Option<&Trend>,
    account: Option<&Account>,
    now: f64,
) -> String {
    let mut out = String::new();
    for (n, i) in instances.iter().enumerate() {
        let probe = outcome_at(probes, n);
        out.push_str(&box_json(i, probe, trend, now).to_string());
        out.push('\n');
    }
    // What moved in the boxes' power since the last look comes before the money, which closes the look.
    for change in trend.map_or(&[][..], Trend::changes) {
        out.push_str(&event_json(change, now).to_string());
        out.push('\n');
    }
    out.push_str(&money_json(&Runway::of(instances, account), now).to_string());
    out.push('\n');
    out
}

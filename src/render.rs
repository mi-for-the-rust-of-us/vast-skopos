//! The table `vas` prints.

use std::fmt::Write as _;

use crate::error::SkoposError;
use crate::instance::{Account, Instance};
use crate::probe::Probe;

/// `secs` as `2d 3h`, `4h 15m` or `7m`.
///
/// ```
/// use vast_skopos::render::age;
/// assert_eq!((age(30.0), age(4.0 * 3600.0 + 900.0), age(2.0 * 86400.0 + 3.0 * 3600.0)),
///   ("0m".to_owned(), "4h 15m".to_owned(), "2d 3h".to_owned()));
/// ```
#[must_use]
pub fn age(secs: f64) -> String {
    // CAST: f64 → u64, a non-negative age in seconds, truncated to whole seconds on purpose
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    let s = secs.max(0.0) as u64;
    let (d, h, m) = (s / 86_400, (s % 86_400) / 3600, (s % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

/// A cell, or a dash.
fn or_dash(v: Option<String>) -> String {
    v.unwrap_or_else(|| String::from("-"))
}

/// The status cell: the actual status, with the intended one when they differ.
fn status(i: &Instance) -> String {
    match (i.actual_status.as_deref(), i.intended_status.as_deref()) {
        (Some(a), Some(want)) if a != want => format!("{a} -> {want}"),
        // BORROW: explicit to_owned -- the cell is owned by the row
        (Some(a), _) => a.to_owned(),
        (None, Some(want)) => format!("? -> {want}"),
        (None, None) => String::from("-"),
    }
}

/// The GPU-utilisation and memory cells from a probe.
fn gpu_cells(probe: &Probe) -> (String, String) {
    let util = probe
        .gpus
        .iter()
        .map(|g| format!("{}%", g.util_pct))
        .collect::<Vec<_>>()
        .join(" ");
    let mem = probe
        .gpus
        .iter()
        .map(|g| {
            format!(
                "{:.1}/{:.0}G",
                mib_to_gib(g.mem_used_mib),
                mib_to_gib(g.mem_total_mib)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    (util, mem)
}

/// MiB → GiB, for display.
fn mib_to_gib(mib: u64) -> f64 {
    // CAST: u64 → f64, a MiB count far below 2^53, for a one-decimal display
    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    let v = mib as f64 / 1024.0;
    v
}

/// The table: one row per instance, then the money line.
///
/// `probes` is aligned with `instances` (`None`: not probed, e.g. not running); `with_ssh` shows
/// the probe columns even when no box was probed; `now` is Unix seconds.
#[must_use]
pub fn table(
    instances: &[Instance],
    probes: &[Option<Result<Probe, SkoposError>>],
    with_ssh: bool,
    account: Option<&Account>,
    now: f64,
) -> String {
    let mut header = vec![
        "ID", "LABEL", "GPU", "WHERE", "STATUS", "$/H", "AGE", "COST~", "SSH",
    ];
    if with_ssh {
        header.extend(["GPU% (ssh)", "VRAM", "LAST LOG / SSH ERROR"]);
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (n, i) in instances.iter().enumerate() {
        let gpu = match (i.num_gpus, i.gpu_name.as_deref()) {
            (Some(k), Some(g)) => format!("{k}x {g}"),
            // BORROW: explicit to_owned -- the cell is owned by the row
            (None, Some(g)) => g.to_owned(),
            (Some(k), None) => format!("{k}x ?"),
            (None, None) => String::from("-"),
        };
        let mut row = vec![
            i.id.to_string(),
            or_dash(i.label.clone()),
            gpu,
            or_dash(i.geolocation.clone()),
            status(i),
            or_dash(i.dph_total.map(|d| format!("{d:.3}"))),
            or_dash(i.age_secs(now).map(age)),
            or_dash(i.cost_so_far(now).map(|c| format!("{c:.2}"))),
            or_dash(i.ssh_endpoint().map(|(h, p)| format!("{h}:{p}"))),
        ];
        if with_ssh {
            match probes.get(n).and_then(Option::as_ref) {
                Some(Ok(p)) => {
                    let (util, mem) = gpu_cells(p);
                    row.extend([util, mem, or_dash(p.last_log.clone())]);
                }
                Some(Err(e)) => row.extend([String::from("?"), String::from("?"), e.to_string()]),
                None => row.extend([String::from("-"), String::from("-"), String::from("-")]),
            }
        }
        rows.push(row);
    }
    let mut widths: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for row in &rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let mut out = String::new();
    let line = |cells: &[String], out: &mut String| {
        let last = cells.len().saturating_sub(1);
        for (k, (cell, w)) in cells.iter().zip(&widths).enumerate() {
            if k == last {
                let _ = write!(out, "{cell}");
            } else {
                let _ = write!(out, "{cell:<w$}  ");
            }
        }
        out.push('\n');
    };
    line(
        // BORROW: explicit to_owned -- the header cells join the owned rows
        &header.iter().map(|h| (*h).to_owned()).collect::<Vec<_>>(),
        &mut out,
    );
    for row in &rows {
        line(row, &mut out);
    }
    if instances.is_empty() {
        out.push_str("(no instances)\n");
    }
    out.push_str(&money_line(instances, account));
    out
}

/// The credit, the burn rate of the running instances, and the hours that credit buys.
///
/// ```
/// use vast_skopos::{instance::{Account, Instance}, render::money_line};
/// let i = Instance::from_json(&serde_json::json!(
///   {"id": 1, "actual_status": "running", "dph_total": 0.5})).unwrap();
/// let a = Account::from_json(&serde_json::json!({"credit": 5.0}));
/// assert_eq!(money_line(&[i], Some(&a)),
///   "credit $5.00 | burn $0.500/h over 1 running | ~10.0 h left at this rate\n");
/// ```
#[must_use]
pub fn money_line(instances: &[Instance], account: Option<&Account>) -> String {
    let running: Vec<&Instance> = instances.iter().filter(|i| i.is_running()).collect();
    // A fold from +0.0, not `sum()`: an empty f64 sum is -0.0, which would print as `$-0.000`.
    let burn = running
        .iter()
        .filter_map(|i| i.dph_total)
        .fold(0.0_f64, |acc, d| acc + d);
    let credit = account.and_then(|a| a.credit);
    let mut s = format!(
        "credit {} | burn ${burn:.3}/h over {} running",
        credit.map_or_else(|| String::from("?"), |c| format!("${c:.2}")),
        running.len()
    );
    if let Some(c) = credit
        && burn > 0.0
    {
        let _ = write!(s, " | ~{:.1} h left at this rate", c / burn);
    }
    s.push('\n');
    s
}

//! The tables `vas` prints: the instances (with the money line), and the offers (with their
//! footer).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::error::SkoposError;
use crate::instance::{Account, Instance};
use crate::offer::{OFFER_CAP, Offer, OfferChanges, OfferPage, OfferQuery, OfferSort};
use crate::probe::{LogReading, Probe};

/// `secs` in whole seconds, truncated: a negative or `NaN` count reads 0, an enormous one saturates.
pub(crate) const fn whole_secs(secs: f64) -> u64 {
    // CAST: f64 → u64, a non-negative time in seconds, truncated to whole seconds on purpose
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    let s = secs.max(0.0) as u64;
    s
}

/// `secs` as `2d 3h`, `4h 15m` or `7m`.
///
/// ```
/// use vast_skopos::render::age;
/// assert_eq!((age(30.0), age(4.0 * 3600.0 + 900.0), age(2.0 * 86400.0 + 3.0 * 3600.0)),
///   ("0m".to_owned(), "4h 15m".to_owned(), "2d 3h".to_owned()));
/// ```
#[must_use]
pub fn age(secs: f64) -> String {
    let s = whole_secs(secs);
    let (d, h, m) = (s / 86_400, (s % 86_400) / 3600, (s % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

/// The time of day of `unix_secs` in UTC, as `15:42:07 UTC` (the date is not shown: the footer that
/// carries it is read within minutes, since offers vanish within minutes).
///
/// ```
/// use vast_skopos::render::clock;
/// assert_eq!(clock(0.0), "00:00:00 UTC");
/// assert_eq!(clock(86_400.0 + 15.0 * 3600.0 + 42.0 * 60.0 + 7.9), "15:42:07 UTC");
/// assert_eq!(clock(-5.0), "00:00:00 UTC");
/// ```
#[must_use]
pub fn clock(unix_secs: f64) -> String {
    let s = whole_secs(unix_secs) % 86_400;
    format!("{:02}:{:02}:{:02} UTC", s / 3600, (s % 3600) / 60, s % 60)
}

/// A cell, or a dash.
fn or_dash(v: Option<String>) -> String {
    v.unwrap_or_else(|| String::from("-"))
}

/// The characters kept in a cell of someone else's text (a GPU name, a location): one host's long
/// string must not widen every row.
const TEXT_WIDTH: usize = 40;

/// `text`, at most [`TEXT_WIDTH`] characters; a longer one loses its tail to an ellipsis (tested
/// through `offers_table`, in `tests/render.rs`).
fn capped(text: &str) -> String {
    if text.chars().count() > TEXT_WIDTH {
        let mut cut: String = text.chars().take(TEXT_WIDTH).collect();
        cut.push('…');
        cut
    } else {
        // BORROW: explicit to_owned -- the cell is owned by the row
        text.to_owned()
    }
}

/// The GPU cell: `1x RTX 5090`, with a `?` or a dash for what is missing.
fn gpu_cell(num_gpus: Option<u64>, gpu_name: Option<&str>) -> String {
    match (num_gpus, gpu_name.map(capped)) {
        (Some(k), Some(g)) => format!("{k}x {g}"),
        (None, Some(g)) => g,
        (Some(k), None) => format!("{k}x ?"),
        (None, None) => String::from("-"),
    }
}

/// `header` and `rows` as columns two spaces apart, one line each; the last column is not padded.
fn layout(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut widths: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for row in rows {
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
    for row in rows {
        line(row, &mut out);
    }
    out
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

/// The path characters kept in the log cell; a longer path loses its head.
const PATH_WIDTH: usize = 60;

/// The last two non-empty components of a path on the box (`run_s0/train.log`); when longer than
/// 60 characters, its last 60, after an ellipsis.
///
/// So a glob whose varying part is one of the last two components (`runs/*/train.log`) says which
/// run wrote the line; with the run deeper (`runs/*/logs/train.log`), it is not shown.
///
/// ```
/// use vast_skopos::render::short_path;
/// assert_eq!(short_path("/root/data/runs/me36_ab_s0_d1/train.log"), "me36_ab_s0_d1/train.log");
/// assert_eq!(short_path("/root/runs//train.log"), "runs/train.log");
/// assert_eq!(short_path("/r/s0/"), "r/s0");
/// assert_eq!((short_path("/train.log"), short_path("train.log")), ("train.log".to_owned(), "train.log".to_owned()));
/// assert_eq!(short_path("//"), "//");
/// let cut = short_path(&format!("/r/{}/train.log", "d".repeat(80)));
/// assert!(cut.starts_with('…') && cut.ends_with("d/train.log") && cut.chars().count() == 61);
/// ```
#[must_use]
pub fn short_path(path: &str) -> String {
    let mut parts = path.rsplit('/').filter(|c| !c.is_empty());
    let short = match (parts.next(), parts.next()) {
        (Some(file), Some(dir)) => format!("{dir}/{file}"),
        // BORROW: explicit to_owned -- the cell is owned by the row
        (Some(file), None) => file.to_owned(),
        // A path with no named component (`/`, `//`, empty) is shown as it is.
        // BORROW: explicit to_owned -- the cell is owned by the row
        (None, _) => path.to_owned(),
    };
    let n = short.chars().count();
    if n > PATH_WIDTH {
        let mut cut = String::from("…");
        cut.extend(short.chars().skip(n - PATH_WIDTH));
        cut
    } else {
        short
    }
}

/// The log cell: which file, and its last line; or why there is none.
///
/// ```
/// use vast_skopos::{probe::LogReading, render::log_cell};
/// assert_eq!(log_cell(None), "-");
/// assert_eq!(log_cell(Some(&LogReading::NoMatch)), "(no file matches)");
/// assert_eq!(log_cell(Some(&LogReading::NotAFile { file: "/r/s1".to_owned() })),
///     "r/s1: (not a file)");
/// assert_eq!(log_cell(Some(&LogReading::Empty { file: "/r/s1/train.log".to_owned() })),
///     "s1/train.log: (empty)");
/// assert_eq!(log_cell(Some(&LogReading::Line {
///     file: "/r/s1/train.log".to_owned(), line: "e41".to_owned() })), "s1/train.log: e41");
/// assert_eq!(log_cell(Some(&LogReading::Unreadable)), "?");
/// ```
#[must_use]
pub fn log_cell(log: Option<&LogReading>) -> String {
    match log {
        None => String::from("-"),
        Some(LogReading::NoMatch) => String::from("(no file matches)"),
        Some(LogReading::NotAFile { file }) => format!("{}: (not a file)", short_path(file)),
        Some(LogReading::Empty { file }) => format!("{}: (empty)", short_path(file)),
        Some(LogReading::Line { file, line }) => format!("{}: {line}", short_path(file)),
        Some(LogReading::Unreadable) => String::from("?"),
    }
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
/// the probe columns even when no box was probed; `wide` adds the machine and host ids; `now` is
/// Unix seconds.
#[must_use]
pub fn table(
    instances: &[Instance],
    probes: &[Option<Result<Probe, SkoposError>>],
    with_ssh: bool,
    wide: bool,
    account: Option<&Account>,
    now: f64,
) -> String {
    let mut header = vec![
        "ID", "LABEL", "GPU", "WHERE", "STATUS", "$/H", "AGE", "COST~", "SSH",
    ];
    if wide {
        header.extend(["MACHINE", "HOST"]);
    }
    if with_ssh {
        header.extend(["GPU% (ssh)", "VRAM", "LAST LOG / SSH ERROR"]);
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (n, i) in instances.iter().enumerate() {
        let mut row = vec![
            i.id.to_string(),
            or_dash(i.label.clone()),
            gpu_cell(i.num_gpus, i.gpu_name.as_deref()),
            or_dash(i.geolocation.as_deref().map(capped)),
            status(i),
            or_dash(i.dph_total.map(|d| format!("{d:.3}"))),
            or_dash(i.age_secs(now).map(age)),
            or_dash(i.cost_so_far(now).map(|c| format!("{c:.2}"))),
            or_dash(i.ssh_endpoint().map(|(h, p)| format!("{h}:{p}"))),
        ];
        if wide {
            row.extend([
                or_dash(i.machine_id.map(|m| m.to_string())),
                or_dash(i.host_id.map(|h| h.to_string())),
            ]);
        }
        if with_ssh {
            match probes.get(n).and_then(Option::as_ref) {
                Some(Ok(p)) => {
                    let (util, mem) = gpu_cells(p);
                    row.extend([util, mem, log_cell(p.log.as_ref())]);
                }
                Some(Err(e)) => row.extend([String::from("?"), String::from("?"), e.to_string()]),
                None => row.extend([String::from("-"), String::from("-"), String::from("-")]),
            }
        }
        rows.push(row);
    }
    let mut out = layout(&header, &rows);
    if instances.is_empty() {
        out.push_str("(no instances)\n");
    }
    out.push_str(&money_line(instances, account));
    out
}

/// The account's money over the fleet: what the money line prints and the compact output carries.
///
/// The burn counts the instances the API reads as `running` AND those it was asked to run
/// (`intended_status`) that read otherwise (`offline`, `loading`, ...): whether those bill is not
/// known from here, and an under-counted burn overstates the runway.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Money {
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

impl Money {
    /// The money of `instances` under `account`.
    ///
    /// ```
    /// use vast_skopos::{instance::{Account, Instance}, render::Money};
    /// let row = |id, actual: &str| Instance::from_json(&serde_json::json!(
    ///   {"id": id, "actual_status": actual, "intended_status": "running", "dph_total": 0.5})).unwrap();
    /// let a = Account::from_json(&serde_json::json!({"credit": 5.0}));
    /// let m = Money::of(&[row(1, "running"), row(2, "offline")], Some(&a));
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

    /// The hours the credit lasts at this burn; `None` without a credit or without a burn.
    #[must_use]
    pub fn hours_left(&self) -> Option<f64> {
        self.credit
            .filter(|_| self.burn > 0.0)
            .map(|c| c / self.burn)
    }

    /// The money line, ending in a newline (see [`money_line`]).
    #[must_use]
    pub fn line(&self) -> String {
        let mut s = format!(
            "credit {} | burn ${:.3}/h over {} running",
            self.credit
                .map_or_else(|| String::from("?"), |c| format!("${c:.2}")),
            self.burn,
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

/// The credit, the burn rate of the instances that bill or may, and the hours that credit buys.
///
/// The burn counts the instances the API reads as `running` AND those it was asked to run
/// (`intended_status`) that read otherwise (`offline`, `loading`, ...): whether those bill is not
/// known from here, and an under-counted burn overstates the runway. They are named after the
/// running ones, by state, and the line says so.
///
/// ```
/// use vast_skopos::{instance::{Account, Instance}, render::money_line};
/// let row = |id, actual: &str, intended: &str| Instance::from_json(&serde_json::json!(
///   {"id": id, "actual_status": actual, "intended_status": intended, "dph_total": 0.5})).unwrap();
/// let a = Account::from_json(&serde_json::json!({"credit": 5.0}));
/// assert_eq!(money_line(&[row(1, "running", "running")], Some(&a)),
///   "credit $5.00 | burn $0.500/h over 1 running | ~10.0 h left at this rate\n");
/// // A box the API reads offline but was asked to run is counted, and named.
/// let fleet = [row(1, "running", "running"), row(2, "offline", "running"), row(3, "exited", "stopped")];
/// assert_eq!(money_line(&fleet, Some(&a)),
///   "credit $5.00 | burn $1.000/h over 1 running + 1 offline (meant to run, billing unknown) \
///    | ~5.0 h left at this rate\n");
/// ```
#[must_use]
pub fn money_line(instances: &[Instance], account: Option<&Account>) -> String {
    Money::of(instances, account).line()
}

/// The offers table's header.
const OFFER_HEADER: [&str; 12] = [
    "ID",
    "GPU",
    "VRAM",
    "$/H",
    "CUDA",
    "DISK",
    "UP/DOWN Mb/s",
    "REL %",
    "MAX DAYS",
    "HOST",
    "VERIFICATION",
    "WHERE",
];

/// One offer's cells, in [`OFFER_HEADER`]'s order.
fn offer_row(o: &Offer) -> Vec<String> {
    let whole = |v: Option<f64>| or_dash(v.map(|x| format!("{x:.0}")));
    let bandwidth = match (o.inet_up, o.inet_down) {
        (None, None) => String::from("-"),
        (up, down) => format!("{}/{}", whole(up), whole(down)),
    };
    vec![
        o.id.to_string(),
        gpu_cell(o.num_gpus, o.gpu_name.as_deref()),
        or_dash(o.gpu_ram.map(|mb| format!("{:.0}G", mb / 1024.0))),
        or_dash(o.dph_total.map(|d| format!("{d:.3}"))),
        or_dash(o.cuda_max_good.map(|c| format!("{c:.1}"))),
        or_dash(o.disk_space.map(|gb| format!("{gb:.0}G"))),
        bandwidth,
        or_dash(o.reliability.map(|r| format!("{:.2}", r * 100.0))),
        or_dash(o.duration.map(|s| format!("{:.1}", s / 86_400.0))),
        or_dash(o.host_id.map(|h| h.to_string())),
        or_dash(o.verification.as_deref().map(capped)),
        or_dash(o.geolocation.as_deref().map(capped)),
    ]
}

/// The offers table: the first `top` of `offers`, in their order; `(no offers)` when there are
/// none.
///
/// ```
/// use vast_skopos::{offer::Offer, render::offers_table};
/// let o = Offer::from_json(&serde_json::json!({"id": 44614424, "gpu_name": "RTX 5090",
///   "num_gpus": 1, "gpu_ram": 32607, "dph_total": 0.54018, "cuda_max_good": 13.0,
///   "disk_space": 691.0, "inet_up": 38.4, "reliability2": 0.9961118, "duration": 6442672.9,
///   "host_id": 410852, "verification": "verified", "geolocation": "United States, US"})).unwrap();
/// let text = offers_table(&[o.clone(), o], 1);
/// assert_eq!(text.lines().count(), 2, "the header and the one row asked for");
/// let row: Vec<&str> = text.lines().nth(1).unwrap().split_whitespace().collect();
/// assert_eq!(row, ["44614424", "1x", "RTX", "5090", "32G", "0.540", "13.0", "691G", "38/-",
///   "99.61", "74.6", "410852", "verified", "United", "States,", "US"]);
/// assert!(offers_table(&[], 20).ends_with("(no offers)\n"));
/// ```
#[must_use]
pub fn offers_table(offers: &[Offer], top: usize) -> String {
    let rows: Vec<Vec<String>> = offers.iter().take(top).map(offer_row).collect();
    let mut out = layout(&OFFER_HEADER, &rows);
    if offers.is_empty() {
        out.push_str("(no offers)\n");
    }
    out
}

/// The column of [`OFFER_HEADER`] that holds the price.
const PRICE_COLUMN: usize = 3;

/// The offers table of a watch: [`offers_table`] with a first column that marks `+` an offer that
/// was not in the previous look, and a price that moved read `0.594 (was 0.575)`.
///
/// ```
/// use vast_skopos::{offer::{Offer, changes}, render::offers_table_marked};
/// let offer = |id: u64, p: f64| Offer::from_json(&serde_json::json!({"id": id, "dph_total": p})).unwrap();
/// let before = [offer(1, 0.5), offer(2, 0.6)];
/// let now = [offer(3, 0.4), offer(1, 0.55), offer(2, 0.6)];
/// let text = offers_table_marked(&now, 20, &changes(&before, &now));
/// let rows: Vec<Vec<&str>> = text.lines().skip(1).map(|l| l.split_whitespace().collect()).collect();
/// assert_eq!((rows[0][0], rows[0][1], rows[0][4]), ("+", "3", "0.400"));
/// assert_eq!(&rows[1][..5], ["1", "-", "-", "0.550", "(was"]);
/// assert_eq!(&rows[2][..2], ["2", "-"], "an unchanged offer has no mark");
/// ```
#[must_use]
pub fn offers_table_marked(offers: &[Offer], top: usize, changes: &OfferChanges) -> String {
    let mut header: Vec<&str> = vec![""];
    header.extend(OFFER_HEADER);
    let rows: Vec<Vec<String>> = offers
        .iter()
        .take(top)
        .map(|o| {
            let mut cells = offer_row(o);
            let mark = if changes.appeared.iter().any(|a| a.id == o.id) {
                "+"
            } else {
                ""
            };
            if let Some(r) = changes.repriced.iter().find(|r| r.offer.id == o.id)
                && let Some(cell) = cells.get_mut(PRICE_COLUMN)
            {
                *cell = format!("{cell} (was {:.3})", r.was);
            }
            cells.insert(0, String::from(mark));
            cells
        })
        .collect();
    let mut out = layout(&header, &rows);
    if offers.is_empty() {
        out.push_str("(no offers)\n");
    }
    out
}

/// At most this many offers are listed per kind of change; the rest are counted.
const CHANGE_LINES: usize = 10;

/// An offer in one line: `46579456  1x RTX 5090  0.554 $/h  California, US`.
fn offer_brief(o: &Offer) -> String {
    format!(
        "{}  {}  {} $/h  {}",
        o.id,
        gpu_cell(o.num_gpus, o.gpu_name.as_deref()),
        or_dash(o.dph_total.map(|d| format!("{d:.3}"))),
        or_dash(o.geolocation.as_deref().map(capped)),
    )
}

/// `2 new, 1 repriced, 3 gone`, or `no change`.
fn change_counts(changes: &OfferChanges) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !changes.appeared.is_empty() {
        parts.push(format!("{} new", changes.appeared.len()));
    }
    if !changes.repriced.is_empty() {
        parts.push(format!("{} repriced", changes.repriced.len()));
    }
    if !changes.gone.is_empty() {
        parts.push(format!("{} gone", changes.gone.len()));
    }
    if parts.is_empty() {
        String::from("no change")
    } else {
        parts.join(", ")
    }
}

/// The lines of one kind of change: `mark` and `describe(item)` for the first [`CHANGE_LINES`],
/// then how many more.
fn change_lines<T>(out: &mut String, mark: char, items: &[T], describe: impl Fn(&T) -> String) {
    for item in items.iter().take(CHANGE_LINES) {
        let _ = writeln!(out, "{mark} {}", describe(item));
    }
    if items.len() > CHANGE_LINES {
        let _ = writeln!(out, "{mark} ... and {} more", items.len() - CHANGE_LINES);
    }
}

/// vast.ai answers at most 64 rows per search: an offer may only have entered or left them.
const CUT_NOTE: &str = "(vast.ai caps a search at 64 rows: an offer may only have entered or left them; narrow the search)\n";

/// What moved since the previous look, to print under the table of a watch.
///
/// The table already marks the new offers and shows the changed prices, so this is the counts,
/// then the offers that are gone (a table cannot show them). `cut` is whether the search may have
/// been cut ([`crate::offer::OfferPage::maybe_cut`]).
///
/// ```
/// use vast_skopos::{offer::{Offer, changes}, render::changes_note};
/// let offer = |id: u64, p: f64| Offer::from_json(&serde_json::json!(
///   {"id": id, "dph_total": p, "gpu_name": "RTX 5090", "num_gpus": 1, "geolocation": "Spain, ES"})).unwrap();
/// let before = [offer(1, 0.5), offer(2, 0.6)];
/// let now = [offer(1, 0.5), offer(3, 0.7)];
/// assert_eq!(changes_note(&changes(&before, &now), false),
///   "since the last look: 1 new, 1 gone\n- 2  1x RTX 5090  0.600 $/h  Spain, ES\n");
/// assert_eq!(changes_note(&changes(&now, &now), false), "since the last look: no change\n");
/// ```
#[must_use]
pub fn changes_note(changes: &OfferChanges, cut: bool) -> String {
    let mut out = format!("since the last look: {}\n", change_counts(changes));
    change_lines(&mut out, '-', &changes.gone, offer_brief);
    if cut && !(changes.appeared.is_empty() && changes.gone.is_empty()) {
        out.push_str(CUT_NOTE);
    }
    out
}

/// The look of `vas offers --watch --changes`: no table, only what moved.
///
/// The first look (no `changes`) is the baseline; later ones name every change, a line each (`+`
/// new, `~` repriced, `-` gone), or say `no change` in the one line. `offers` is how many offers
/// the look kept, `now` the time of the search (Unix seconds).
///
/// ```
/// use vast_skopos::{offer::{Offer, changes}, render::changes_only};
/// let offer = |id: u64, p: f64| Offer::from_json(&serde_json::json!(
///   {"id": id, "dph_total": p, "gpu_name": "RTX 5090", "num_gpus": 1, "geolocation": "Spain, ES"})).unwrap();
/// assert_eq!(changes_only(None, 2, false, 0.0),
///   "searched 00:00:00 UTC: 2 offers (the first look: what moves follows)\n");
/// let before = [offer(1, 0.5), offer(2, 0.6)];
/// let now = [offer(1, 0.55), offer(3, 0.7)];
/// assert_eq!(changes_only(Some(&changes(&before, &now)), 2, false, 3661.0),
///   "searched 01:01:01 UTC: 2 offers, 1 new, 1 repriced, 1 gone\n\
///    + 3  1x RTX 5090  0.700 $/h  Spain, ES\n\
///    ~ 1  1x RTX 5090  0.500 -> 0.550 $/h  Spain, ES\n\
///    - 2  1x RTX 5090  0.600 $/h  Spain, ES\n");
/// assert_eq!(changes_only(Some(&changes(&now, &now)), 2, false, 3661.0),
///   "searched 01:01:01 UTC: 2 offers, no change\n");
/// ```
#[must_use]
pub fn changes_only(changes: Option<&OfferChanges>, offers: usize, cut: bool, now: f64) -> String {
    let at = clock(now);
    let noun = if offers == 1 { "offer" } else { "offers" };
    let Some(changes) = changes else {
        return format!("searched {at}: {offers} {noun} (the first look: what moves follows)\n");
    };
    let mut out = format!(
        "searched {at}: {offers} {noun}, {}\n",
        change_counts(changes)
    );
    change_lines(&mut out, '+', &changes.appeared, offer_brief);
    change_lines(&mut out, '~', &changes.repriced, |r| {
        format!(
            "{}  {}  {:.3} -> {} $/h  {}",
            r.offer.id,
            gpu_cell(r.offer.num_gpus, r.offer.gpu_name.as_deref()),
            r.was,
            or_dash(r.offer.dph_total.map(|d| format!("{d:.3}"))),
            or_dash(r.offer.geolocation.as_deref().map(capped)),
        )
    });
    change_lines(&mut out, '-', &changes.gone, offer_brief);
    if cut && !(changes.appeared.is_empty() && changes.gone.is_empty()) {
        out.push_str(CUT_NOTE);
    }
    out
}

/// The offers footer: counts, order, constraints and the storage priced in; then any notes.
///
/// The notes say when the answer may be cut (vast.ai's cap, or the server's own `truncated`), how
/// many rows the re-check dropped, and how many were skipped for want of an id. `kept` is how many
/// offers passed the re-check ([`crate::offer::select`]), `shown` how many the table shows. `now`
/// (Unix seconds) is the time of the search, named on the first line: the list is a snapshot of a
/// market that moves within minutes.
///
/// ```
/// use vast_skopos::{offer::{OfferPage, OfferQuery, OfferSort}, render::offers_line};
/// let page = OfferPage::from_json(&serde_json::json!(
///   {"offers": [{"id": 1}, {"id": 2}, {"id": 3}], "truncated": false})).unwrap();
/// let mut q = OfferQuery::default();
/// q.disk = Some(50);
/// assert_eq!(offers_line(&page, 2, 1, &q, OfferSort::Price, 56_527.0),
///   "1 of 2 offers shown, cheapest first | disk >= 50 GB | $/h includes 50 GB of storage \
///    | searched 15:42:07 UTC\n1 dropped here: a field contradicts a constraint\n");
/// let one = OfferPage::from_json(&serde_json::json!({"offers": [{"id": 1}]})).unwrap();
/// assert_eq!(offers_line(&one, 1, 1, &OfferQuery::default(), OfferSort::Up, 0.0),
///   "1 offer, fastest upload first | no constraint | $/h includes 5 GB of storage \
///    | searched 00:00:00 UTC\n");
/// ```
#[must_use]
pub fn offers_line(
    page: &OfferPage,
    kept: usize,
    shown: usize,
    query: &OfferQuery,
    sort: OfferSort,
    now: f64,
) -> String {
    let count = if shown < kept {
        format!("{shown} of {kept} offers shown")
    } else if kept == 1 {
        String::from("1 offer")
    } else {
        format!("{kept} offers")
    };
    let mut s = format!(
        "{count}, {} | {} | $/h includes {} GB of storage | searched {}\n",
        sort.words(),
        query.summary(),
        query.storage_gb(),
        clock(now)
    );
    let mut notes: Vec<String> = Vec::new();
    if page.maybe_cut() {
        notes.push(if page.returned >= OFFER_CAP {
            format!(
                "{} returned, vast.ai's cap: the list may be cut (narrow the search)",
                page.returned
            )
        } else {
            String::from("vast.ai says the list was cut (narrow the search)")
        });
    }
    let dropped = page.offers.len().saturating_sub(kept);
    if dropped > 0 {
        notes.push(format!(
            "{dropped} dropped here: a field contradicts a constraint"
        ));
    }
    if page.skipped > 0 {
        notes.push(format!("{} without an id skipped", page.skipped));
    }
    if !notes.is_empty() {
        s.push_str(&notes.join(" | "));
        s.push('\n');
    }
    s
}

//! The tables `vas` prints: the instances (with the money line), the offers (with their footer),
//! and what a watch adds to them (the marks, and the notes on what moved).

use std::fmt::Write as _;

use crate::instance::{Account, Instance, shortened, whole_secs};
use crate::offer::{OFFER_CAP, Offer, OfferChanges, OfferPage, OfferQuery, OfferSort};
use crate::probe::{LogReading, Probe, ProbeOutcome, outcome_at};
use crate::runway::Runway;
use crate::trend::Trend;

/// `secs` as `2d 3h`, `4h 15m` or `7m`.
///
/// ```
/// use vast_skopos::render::age;
/// assert_eq!((age(30.0), age(4.0 * 3600.0 + 900.0), age(2.0 * 86400.0 + 3.0 * 3600.0)),
///   ("0m".to_owned(), "4h 15m".to_owned(), "2d 3h".to_owned()));
/// ```
#[doc(hidden)]
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
#[must_use]
pub(crate) fn clock(unix_secs: f64) -> String {
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

/// `text`; when it has more than [`TEXT_WIDTH`] characters, its first [`TEXT_WIDTH`] and an ellipsis
/// (tested through `offers_table`, in `tests/render.rs`).
fn capped(text: &str) -> String {
    // BORROW: explicit to_owned -- the cell is owned by the row
    shortened(text, TEXT_WIDTH).unwrap_or_else(|| text.to_owned())
}

/// A price cell: dollars per hour to the thousandth, or a dash.
fn price_cell(price: Option<f64>) -> String {
    or_dash(price.map(|d| format!("{d:.3}")))
}

/// A cell of a host's text (a location, a verdict), capped, or a dash.
fn text_cell(text: Option<&str>) -> String {
    or_dash(text.map(capped))
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
    push_line(&mut out, header.iter().copied(), &widths);
    for row in rows {
        push_line(&mut out, row.iter().map(String::as_str), &widths);
    }
    out
}

/// One line of [`layout`]: the cells padded to `widths`, the last one not.
fn push_line<'a>(
    out: &mut String,
    cells: impl ExactSizeIterator<Item = &'a str>,
    widths: &[usize],
) {
    let last = cells.len().saturating_sub(1);
    for (k, (cell, w)) in cells.zip(widths).enumerate() {
        if k == last {
            out.push_str(cell);
        } else {
            let _ = write!(out, "{cell:<w$}  ");
        }
    }
    out.push('\n');
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
#[doc(hidden)]
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
#[doc(hidden)]
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

/// Which optional columns [`table`] shows. `TableOptions::new()` is the plain table; the other
/// columns are asked for by name, so no call site reads as two bare booleans.
///
/// ```
/// use vast_skopos::{render::TableOptions, trend::Trend};
/// let options = TableOptions::new().ssh(true).wide(true);
/// assert!(options.ssh && options.wide && options.trend.is_none());
/// assert_eq!(TableOptions::default(), TableOptions::new());
/// let history = Trend::default();
/// assert!(TableOptions::new().ssh(true).trend(&history).trend.is_some());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct TableOptions<'a> {
    /// Show the probe columns (`GPU% (ssh)`, `VRAM`, `LAST LOG / SSH ERROR`), even when no box was
    /// probed.
    pub ssh: bool,
    /// Show each box's machine id and host id.
    pub wide: bool,
    /// The samples a watch has kept: adds a `TREND` column after `VRAM` (with the probe columns).
    pub trend: Option<&'a Trend>,
}

impl<'a> TableOptions<'a> {
    /// The plain table: no probe columns, no ids, no trend.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ssh: false,
            wide: false,
            trend: None,
        }
    }

    /// With the last GPU samples of each box (see [`Trend`]), as a `TREND` column after `VRAM`;
    /// it shows with the probe columns.
    #[must_use]
    pub const fn trend(mut self, trend: &'a Trend) -> Self {
        self.trend = Some(trend);
        self
    }

    /// With the probe columns (`on`), or without.
    #[must_use]
    pub const fn ssh(mut self, on: bool) -> Self {
        self.ssh = on;
        self
    }

    /// With the machine and host ids (`on`), or without.
    #[must_use]
    pub const fn wide(mut self, on: bool) -> Self {
        self.wide = on;
        self
    }
}

/// The table: one row per instance, then the money line.
///
/// `probes` holds one outcome per instance, in the instances' order (see [`ProbeOutcome`]); a
/// shorter slice reads as "not probed" for the instances it does not reach. `options` chooses the
/// optional columns, and `now` is Unix seconds.
///
/// ```
/// use vast_skopos::{instance::{Account, Instance}, render::{table, TableOptions}};
/// let i = Instance::from_json(&serde_json::json!({"id": 7, "actual_status": "running",
///   "gpu_name": "RTX 5090", "num_gpus": 1, "dph_total": 0.5, "start_date": 0.0,
///   "machine_id": 9, "host_id": 5})).unwrap();
/// let account = Account::from_json(&serde_json::json!({"credit": 5.0}));
/// let text = table(&[i], &[], Some(&account), TableOptions::new().wide(true), 3600.0);
/// let header: Vec<&str> = text.lines().next().unwrap().split_whitespace().collect();
/// assert_eq!(&header[8..], ["SSH", "MACHINE", "HOST"]);
/// assert!(text.ends_with("credit $5.00 | burn $0.500/h over 1 running | ~10.0 h left at this rate\n"));
/// ```
#[must_use]
pub fn table(
    instances: &[Instance],
    probes: &[ProbeOutcome],
    account: Option<&Account>,
    options: TableOptions<'_>,
    now: f64,
) -> String {
    let TableOptions {
        ssh: with_ssh,
        wide,
        trend,
        ..
    } = options;
    let mut header = vec![
        "ID", "LABEL", "GPU", "WHERE", "STATUS", "$/H", "AGE", "COST~", "SSH",
    ];
    if wide {
        header.extend(["MACHINE", "HOST"]);
    }
    if with_ssh {
        header.extend(["GPU% (ssh)", "VRAM"]);
        if trend.is_some() {
            header.push("TREND");
        }
        header.push("LAST LOG / SSH ERROR");
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (n, i) in instances.iter().enumerate() {
        let mut row = vec![
            i.id.to_string(),
            or_dash(i.label.clone()),
            gpu_cell(i.num_gpus, i.gpu_name.as_deref()),
            text_cell(i.geolocation.as_deref()),
            status(i),
            price_cell(i.dph_total),
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
            let (util, mem, last) = match outcome_at(probes, n) {
                Some(Ok(p)) => {
                    let (util, mem) = gpu_cells(p);
                    (util, mem, log_cell(p.log.as_ref()))
                }
                Some(Err(e)) => (String::from("?"), String::from("?"), e.to_string()),
                None => (String::from("-"), String::from("-"), String::from("-")),
            };
            row.extend([util, mem]);
            if let Some(history) = trend {
                row.push(history.text(i.id));
            }
            row.push(last);
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

/// The money line: the credit, the burn rate of the instances that bill or may, and the hours that
/// credit buys (see [`Runway`], which holds the figures, for what the burn counts).
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
    Runway::of(instances, account).line()
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

/// One offer's cells, in [`OFFER_HEADER`]'s order; `was` is the price it had at the previous look,
/// when it moved, shown beside the current one.
fn offer_row(o: &Offer, was: Option<f64>) -> Vec<String> {
    let whole = |v: Option<f64>| or_dash(v.map(|x| format!("{x:.0}")));
    let bandwidth = match (o.inet_up, o.inet_down) {
        (None, None) => String::from("-"),
        (up, down) => format!("{}/{}", whole(up), whole(down)),
    };
    let price = price_cell(o.dph_total);
    let price = was.map_or_else(|| price.clone(), |old| format!("{price} (was {old:.3})"));
    vec![
        o.id.to_string(),
        gpu_cell(o.num_gpus, o.gpu_name.as_deref()),
        or_dash(o.gpu_ram.map(|mb| format!("{:.0}G", mb / 1024.0))),
        price,
        or_dash(o.cuda_max_good.map(|c| format!("{c:.1}"))),
        or_dash(o.disk_space.map(|gb| format!("{gb:.0}G"))),
        bandwidth,
        or_dash(o.reliability.map(|r| format!("{:.2}", r * 100.0))),
        or_dash(o.duration.map(|s| format!("{:.1}", s / 86_400.0))),
        or_dash(o.host_id.map(|h| h.to_string())),
        text_cell(o.verification.as_deref()),
        text_cell(o.geolocation.as_deref()),
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
    offers_view(offers, top, None)
}

/// The offers table, with a first column of marks and the previous price of the offers that moved
/// when `marks` is given (a watch's look), without them otherwise.
fn offers_view(offers: &[Offer], top: usize, marks: Option<&OfferChanges>) -> String {
    let mut header: Vec<&str> = Vec::new();
    if marks.is_some() {
        header.push("");
    }
    header.extend(OFFER_HEADER);
    let rows: Vec<Vec<String>> = offers
        .iter()
        .take(top)
        .map(|o| {
            let was = marks
                .and_then(|m| m.repriced.iter().find(|r| r.offer.id == o.id))
                .map(|r| r.was);
            let mut cells = offer_row(o, was);
            if let Some(m) = marks {
                let mark = if m.appeared.iter().any(|a| a.id == o.id) {
                    "+"
                } else {
                    ""
                };
                cells.insert(0, String::from(mark));
            }
            cells
        })
        .collect();
    let mut out = layout(&header, &rows);
    if offers.is_empty() {
        out.push_str("(no offers)\n");
    }
    out
}

/// The offers table of a watch: [`offers_table`] with a first column that marks `+` an offer that
/// was not in the previous look, and a price that moved read `0.594 (was 0.575)`.
///
/// ```
/// use vast_skopos::{offer::{Offer, OfferChanges}, render::offers_table_marked};
/// let offer = |id: u64, p: f64| Offer::from_json(&serde_json::json!({"id": id, "dph_total": p})).unwrap();
/// let before = [offer(1, 0.5), offer(2, 0.6)];
/// let now = [offer(3, 0.4), offer(1, 0.55), offer(2, 0.6)];
/// let text = offers_table_marked(&now, 20, &OfferChanges::between(&before, &now));
/// let rows: Vec<Vec<&str>> = text.lines().skip(1).map(|l| l.split_whitespace().collect()).collect();
/// assert_eq!((rows[0][0], rows[0][1], rows[0][4]), ("+", "3", "0.400"));
/// assert_eq!(&rows[1][..5], ["1", "-", "-", "0.550", "(was"]);
/// assert_eq!(&rows[2][..2], ["2", "-"], "an unchanged offer has no mark");
/// ```
#[must_use]
pub fn offers_table_marked(offers: &[Offer], top: usize, changes: &OfferChanges) -> String {
    offers_view(offers, top, Some(changes))
}

/// In the table mode, at most this many gone offers are listed under the table; the rest are
/// counted. `--changes` lists every change: a log that drops ids is of no use.
const NOTE_LINES: usize = 10;

/// An offer in one line, with the `price` text given: `46579456  1x RTX 5090  0.554 $/h  California, US`.
fn offer_brief(o: &Offer, price: &str) -> String {
    format!(
        "{}  {}  {price} $/h  {}",
        o.id,
        gpu_cell(o.num_gpus, o.gpu_name.as_deref()),
        text_cell(o.geolocation.as_deref()),
    )
}

/// An offer in one line, at its current price.
fn brief(o: &Offer) -> String {
    offer_brief(o, &price_cell(o.dph_total))
}

/// `3 new`, or `3 new (2 below the table)`.
fn counted(n: usize, word: &str, below: usize) -> String {
    if below == 0 {
        format!("{n} {word}")
    } else {
        format!("{n} {word} ({below} below the table)")
    }
}

/// `2 new (1 below the table), 1 repriced, 3 gone`, or `no change`.
///
/// `shown` is the rows of the table, when there is one: a new or repriced offer that is not among
/// them has no mark and no changed price on screen, so the counts say how many sit below it.
fn change_counts(changes: &OfferChanges, shown: Option<&[Offer]>) -> String {
    if changes.is_empty() {
        return String::from("no change");
    }
    let below = |id: u64| shown.is_some_and(|rows| !rows.iter().any(|o| o.id == id));
    let mut parts: Vec<String> = Vec::new();
    if !changes.appeared.is_empty() {
        let hidden = changes.appeared.iter().filter(|o| below(o.id)).count();
        parts.push(counted(changes.appeared.len(), "new", hidden));
    }
    if !changes.repriced.is_empty() {
        let hidden = changes
            .repriced
            .iter()
            .filter(|r| below(r.offer.id))
            .count();
        parts.push(counted(changes.repriced.len(), "repriced", hidden));
    }
    if !changes.gone.is_empty() {
        parts.push(format!("{} gone", changes.gone.len()));
    }
    parts.join(", ")
}

/// The lines of one kind of change: `mark` and `describe(item)` for the first `cap` items (all of
/// them without a cap), then how many more.
fn change_lines<T>(
    out: &mut String,
    mark: char,
    items: &[T],
    cap: Option<usize>,
    describe: impl Fn(&T) -> String,
) {
    let listed = cap.map_or(items.len(), |c| c.min(items.len()));
    for item in items.iter().take(listed) {
        let _ = writeln!(out, "{mark} {}", describe(item));
    }
    if listed < items.len() {
        let _ = writeln!(out, "{mark} ... and {} more", items.len() - listed);
    }
}

/// vast.ai answers at most 64 rows per search: an offer may only have entered or left them.
const CUT_NOTE: &str = "(vast.ai caps a search at 64 rows: an offer may only have entered or left them; narrow the search)\n";

/// The note about vast.ai's 64-row cap, when the answer may have been cut AND something entered or
/// left (the only changes the cap can explain); empty otherwise.
fn cut_note(changes: &OfferChanges, page: &OfferPage) -> &'static str {
    if page.maybe_cut() && !(changes.appeared.is_empty() && changes.gone.is_empty()) {
        CUT_NOTE
    } else {
        ""
    }
}

/// What moved since the previous look, to print under the table of a watch.
///
/// The table already marks the new offers and shows the changed prices, so this is the counts
/// (naming how many of the new and repriced sit below the table, where nothing marks them), then
/// the offers that are gone (a table cannot show them). `shown` is the rows the table shows; `page`
/// is the search's answer, whose cap may explain an entry or an exit; `missed` is whether a search
/// failed since the previous look, which then reads `since the last successful look`.
///
/// ```
/// use vast_skopos::{offer::{Offer, OfferChanges, OfferPage}, render::offers_changes_note};
/// let offer = |id: u64, p: f64| Offer::from_json(&serde_json::json!(
///   {"id": id, "dph_total": p, "gpu_name": "RTX 5090", "num_gpus": 1, "geolocation": "Spain, ES"})).unwrap();
/// let page = OfferPage::from_json(&serde_json::json!({"offers": []})).unwrap();
/// let before = [offer(1, 0.5), offer(2, 0.6)];
/// let now = [offer(1, 0.5), offer(3, 0.7)];
/// let moved = OfferChanges::between(&before, &now);
/// assert_eq!(offers_changes_note(&page, &moved, &now, false),
///   "since the last look: 1 new, 1 gone\n- 2  1x RTX 5090  0.600 $/h  Spain, ES\n");
/// // Only the first row is on screen: the new offer is below the table.
/// assert_eq!(offers_changes_note(&page, &moved, &now[..1], true),
///   "since the last successful look: 1 new (1 below the table), 1 gone\n\
///    - 2  1x RTX 5090  0.600 $/h  Spain, ES\n");
/// let steady = OfferChanges::between(&now, &now);
/// assert_eq!(offers_changes_note(&page, &steady, &now, false), "since the last look: no change\n");
/// ```
#[must_use]
pub fn offers_changes_note(
    page: &OfferPage,
    changes: &OfferChanges,
    shown: &[Offer],
    missed: bool,
) -> String {
    let since = if missed {
        "since the last successful look"
    } else {
        "since the last look"
    };
    let mut out = format!("{since}: {}\n", change_counts(changes, Some(shown)));
    change_lines(&mut out, '-', &changes.gone, Some(NOTE_LINES), brief);
    out.push_str(cut_note(changes, page));
    out
}

/// The look of `vas offers --watch --changes`: no table, only what moved.
///
/// The first look (no `changes`) is the baseline; later ones name every change, a line each (`+`
/// new, `~` repriced, `-` gone, none left out), or say `no change` in the one line. `kept` is the
/// offers the look kept, `page` the search's answer (whose cap may explain an entry or an exit),
/// `missed` whether a search failed since the previous look (the line then says so), `now` the time
/// of the search (Unix seconds).
///
/// ```
/// use vast_skopos::{offer::{Offer, OfferChanges, OfferPage}, render::offers_changes_only};
/// let offer = |id: u64, p: f64| Offer::from_json(&serde_json::json!(
///   {"id": id, "dph_total": p, "gpu_name": "RTX 5090", "num_gpus": 1, "geolocation": "Spain, ES"})).unwrap();
/// let page = OfferPage::from_json(&serde_json::json!({"offers": []})).unwrap();
/// let before = [offer(1, 0.5), offer(2, 0.6)];
/// let now = [offer(1, 0.55), offer(3, 0.7)];
/// assert_eq!(offers_changes_only(&page, None, &before, false, 0.0),
///   "searched 00:00:00 UTC: 2 offers (the first look: what moves follows)\n");
/// let moved = OfferChanges::between(&before, &now);
/// assert_eq!(offers_changes_only(&page, Some(&moved), &now, false, 3661.0),
///   "searched 01:01:01 UTC: 2 offers, 1 new, 1 repriced, 1 gone\n\
///    + 3  1x RTX 5090  0.700 $/h  Spain, ES\n\
///    ~ 1  1x RTX 5090  0.500 -> 0.550 $/h  Spain, ES\n\
///    - 2  1x RTX 5090  0.600 $/h  Spain, ES\n");
/// let steady = OfferChanges::between(&now, &now);
/// assert_eq!(offers_changes_only(&page, Some(&steady), &now, true, 3661.0),
///   "searched 01:01:01 UTC: 2 offers, no change (a search failed in between)\n");
/// ```
#[must_use]
pub fn offers_changes_only(
    page: &OfferPage,
    changes: Option<&OfferChanges>,
    kept: &[Offer],
    missed: bool,
    now: f64,
) -> String {
    let offers = kept.len();
    let at = clock(now);
    let noun = if offers == 1 { "offer" } else { "offers" };
    let Some(changes) = changes else {
        return format!("searched {at}: {offers} {noun} (the first look: what moves follows)\n");
    };
    let failed = if missed {
        " (a search failed in between)"
    } else {
        ""
    };
    let mut out = format!(
        "searched {at}: {offers} {noun}, {}{failed}\n",
        change_counts(changes, None)
    );
    change_lines(&mut out, '+', &changes.appeared, None, brief);
    change_lines(&mut out, '~', &changes.repriced, None, |r| {
        let price = format!("{:.3} -> {}", r.was, price_cell(r.offer.dph_total));
        offer_brief(&r.offer, &price)
    });
    change_lines(&mut out, '-', &changes.gone, None, brief);
    out.push_str(cut_note(changes, page));
    out
}

/// The footer of the offers table: counts, order, constraints and the storage priced in; then any
/// notes.
///
/// `page` is the answer, `kept` the offers that passed the re-check ([`crate::offer::select`]) and
/// `top` how many of them the table shows. The notes say when the answer may be cut (vast.ai's cap,
/// or the server's own `truncated`), how many rows repeated an id, how many offers the re-check
/// dropped, and how many rows were skipped for want of an id. `now` (Unix seconds) is the time of the search, named on the first line: the
/// list is a snapshot of a market that moves within minutes.
///
/// ```
/// use vast_skopos::{offer::{OfferPage, OfferQuery, OfferSort}, render::offers_footer};
/// let page = OfferPage::from_json(&serde_json::json!(
///   {"offers": [{"id": 1}, {"id": 2}, {"id": 3}], "truncated": false})).unwrap();
/// let mut q = OfferQuery::default();
/// q.disk = Some(50);
/// // Two of the three offers passed the re-check; the table shows one.
/// assert_eq!(offers_footer(&page, &page.offers[..2], 1, &q, OfferSort::Price, 56_527.0),
///   "1 of 2 offers shown, cheapest first | disk >= 50 GB | $/h includes 50 GB of storage \
///    | searched 15:42:07 UTC\n1 dropped here: a field contradicts a constraint\n");
/// let one = OfferPage::from_json(&serde_json::json!({"offers": [{"id": 1}]})).unwrap();
/// assert_eq!(offers_footer(&one, &one.offers, 20, &OfferQuery::default(), OfferSort::Up, 0.0),
///   "1 offer, fastest upload first | no constraint | $/h includes 5 GB of storage \
///    | searched 00:00:00 UTC\n");
/// ```
#[must_use]
pub fn offers_footer(
    page: &OfferPage,
    kept: &[Offer],
    top: usize,
    query: &OfferQuery,
    sort: OfferSort,
    now: f64,
) -> String {
    let (kept, shown) = (kept.len(), kept.len().min(top));
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
    // `select` keeps one row per id: the rows that repeat an id are not the constraints' doing.
    let distinct = page
        .offers
        .iter()
        .map(|o| o.id)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let repeated = page.offers.len().saturating_sub(distinct);
    if repeated > 0 {
        notes.push(format!(
            "{repeated} repeated id{} skipped",
            if repeated == 1 { "" } else { "s" }
        ));
    }
    let dropped = distinct.saturating_sub(kept);
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

#[cfg(test)]
mod tests {
    use super::clock;

    #[test]
    fn the_clock_reads_the_utc_time_of_day_and_never_fails() {
        assert_eq!(clock(0.0), "00:00:00 UTC");
        assert_eq!(clock(142_927.9), "15:42:07 UTC");
        assert_eq!(clock(86_399.0), "23:59:59 UTC");
        assert_eq!(clock(86_400.0), "00:00:00 UTC", "midnight wraps");
        for odd in [-5.0, f64::NAN, f64::NEG_INFINITY, f64::INFINITY, 1e300] {
            let text = clock(odd);
            assert!(
                text.ends_with(" UTC") && text.len() == "00:00:00 UTC".len(),
                "{odd}: {text}"
            );
        }
        assert_eq!(clock(-5.0), "00:00:00 UTC");
    }
}

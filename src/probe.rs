//! Asking a box directly, over the system `ssh`: what are its GPUs doing, and what did the run
//! last write?
//!
//! The system `ssh` is used rather than an `ssh` library, so the user's keys, agent and
//! `~/.ssh/config` apply unchanged. ONE attempt per call: a box that refuses is reported as such
//! and never retried in a loop (`BatchMode=yes`, `ConnectTimeout=10`), which is a person's
//! decision to make, from the dashboard.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::error::{Result, SkoposError};
use crate::instance::{Instance, is_hidden, shortened};

/// One GPU's reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct GpuReading {
    /// The GPU's index on the box.
    pub index: u32,
    /// Utilisation, percent (0-100), as `nvidia-smi` samples it.
    pub util_pct: u32,
    /// Memory used, MiB.
    pub mem_used_mib: u64,
    /// Memory total, MiB.
    pub mem_total_mib: u64,
    /// The power it draws now, in watts (rounded); `None` when the box did not say.
    pub power_w: Option<u32>,
    /// The power limit set on it, in watts (rounded): a host can set it below the default.
    pub power_limit_w: Option<u32>,
    /// The power limit the card ships with, in watts (575 for an RTX 5090).
    pub power_default_w: Option<u32>,
    /// The SM clock now, in MHz.
    pub sm_mhz: Option<u32>,
    /// The highest SM clock the card can reach, in MHz.
    pub sm_max_mhz: Option<u32>,
    /// Why the clocks are held down: `nvidia-smi`'s bit mask of clock event reasons (`0x4` is the
    /// software power cap, active whenever the card is at its power limit: compare
    /// [`GpuReading::power_limit_w`] with [`GpuReading::power_default_w`]); `None` when the box did not say.
    pub throttle_mask: Option<u64>,
    /// The memory in use that no compute process of the box accounts for, in MiB: what the device
    /// holds (`mem_used_mib`) minus what the processes `nvidia-smi` lists on it hold. `None` when
    /// the box did not give the process list or one of its figures. A few MiB is the driver's own;
    /// gigabytes belong to a process outside the container ([`GpuReading::other_user_mib`]).
    pub other_mem_mib: Option<u64>,
    /// The most power the board accepts, in watts (rounded): `nvidia-smi`'s `power.max_limit`, above
    /// which no limit can be set. Below its model's reference ([`reference_power_w`]) the board itself
    /// is the cap: a 450 W RTX 5090 board read `450W`, like an uncapped card, while idle (2026-10-26).
    /// `None` when the box did not say.
    pub power_max_w: Option<u32>,
    /// The compute processes `nvidia-smi` lists on the GPU, as the box's container sees them. `None`
    /// when the process query did not end with its success line or the GPU's bus id is unknown: an
    /// empty answer is then a failed query, not "no process".
    pub processes: Option<u32>,
}

/// The board power NVIDIA gives for a GPU model (its total graphics power), in watts.
///
/// Keyed by the model's name as vast.ai writes it: what a card of that model draws at full load
/// when nothing holds it lower. Only models with one such figure are listed (an RTX 4060 Ti is 160
/// or 165 W, by its memory); an add-in board may ship above it (a `500/600W` RTX 5090 was seen on
/// 2026-10-25).
pub const REFERENCE_POWER_W: [(&str, u32); 5] = [
    ("RTX 5090", 575),
    ("RTX 5080", 360),
    ("RTX 4090", 450),
    ("RTX 4080", 320),
    ("RTX 3090", 350),
];

/// The reference board power of the GPU model `gpu_name` (vast.ai's name, any ASCII case), in watts;
/// `None` for a model not in [`REFERENCE_POWER_W`].
///
/// ```
/// use vast_skopos::probe::reference_power_w;
/// assert_eq!(reference_power_w("RTX 5090"), Some(575));
/// assert_eq!(reference_power_w("rtx 4090"), Some(450));
/// assert_eq!((reference_power_w("RTX 4060 Ti"), reference_power_w("RTX 5090 D")), (None, None));
/// ```
#[must_use]
pub fn reference_power_w(gpu_name: &str) -> Option<u32> {
    let name = gpu_name.trim();
    REFERENCE_POWER_W
        .iter()
        .find(|(model, _)| model.eq_ignore_ascii_case(name))
        .map(|&(_, watts)| watts)
}

/// The least memory, in MiB, that no process of the box accounts for before it is called the work
/// of someone else: the driver and its contexts hold tens to a few hundred MiB themselves.
pub const OTHER_MIB_MIN: u64 = 1024;

/// MiB → GiB, for display.
pub(crate) fn mib_to_gib(mib: u64) -> f64 {
    // CAST: u64 → f64, a MiB count far below 2^53, for a one-decimal display
    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    let v = mib as f64 / 1024.0;
    v
}

/// The clock event reasons that mean the card is held below what it could do, as `nvidia-smi`
/// names their bits: the software power cap (active whenever the card is at its power limit, which
/// every card reaches under full load), the thermal slowdowns, the hardware slowdown and the power
/// brake. An idle GPU, application clocks and sync boost are not in the list: they are not a
/// restraint on a run.
const THROTTLE_WORDS: [(u64, &str); 5] = [
    (0x4, "power_cap"),
    (0x8, "slowdown"),
    (0x20, "thermal"),
    (0x40, "thermal"),
    (0x80, "power_brake"),
];

impl GpuReading {
    /// Whether the utilisation reads busy (99 % or more) while the card holds no memory to speak of
    /// (under [`OTHER_MIB_MIN`]) and lists no process: an idle card whose counter is stuck, seen three
    /// times (2026-10-15, 10-19, 10-26; on 10-26: 100 %, 2 MiB, 123 W, 37 °C, no process). It reads
    /// like "someone else is on it", which `OTHER` and `VRAM` already deny.
    ///
    /// ```
    /// use vast_skopos::probe::parse_output;
    /// let idle = parse_output("0, 100, 2, 32607\n---vas-power---\n---vas-apps---\n0, 00000000:C1:00.0\n\
    ///                          ---vas-apps-by-gpu---\nvas-apps-ok\n");
    /// assert!(idle.gpus[0].idle_counter());
    /// let busy = parse_output("0, 99, 7700, 32607\n---vas-power---\n---vas-apps---\n0, 00000000:C1:00.0\n\
    ///                          ---vas-apps-by-gpu---\n00000000:C1:00.0, 7600\nvas-apps-ok\n");
    /// assert!(!busy.gpus[0].idle_counter());
    /// ```
    #[must_use]
    pub fn idle_counter(&self) -> bool {
        self.util_pct >= 99 && self.processes == Some(0) && self.mem_used_mib < OTHER_MIB_MIN
    }

    /// The memory held by processes outside the container, in MiB: [`GpuReading::other_mem_mib`]
    /// when it reaches [`OTHER_MIB_MIN`], else `None`.
    ///
    /// On 2026-10-18 a rented RTX 5090 held 24,809 MiB while its only visible process held 8,720:
    /// something outside the container used the rest, and about half of the GPU's time.
    ///
    /// ```
    /// use vast_skopos::probe::parse_output;
    /// let out = "0, 100, 24809, 32607\n---vas-power---\n---vas-apps---\n0, 00000000:C1:00.0\n\
    ///            ---vas-apps-by-gpu---\n00000000:C1:00.0, 8720\nvas-apps-ok\n";
    /// let g = &parse_output(out).gpus[0];
    /// assert_eq!(g.other_mem_mib, Some(16_089));
    /// assert_eq!(g.other_user_mib(), Some(16_089));
    /// ```
    #[must_use]
    pub fn other_user_mib(&self) -> Option<u64> {
        self.other_mem_mib.filter(|mib| *mib >= OTHER_MIB_MIN)
    }

    /// The reasons the clocks are held down, as short words (`power_cap`, `thermal`, `slowdown`,
    /// `power_brake`); `None` when the box did not say, empty when nothing holds the card.
    ///
    /// ```
    /// use vast_skopos::probe::parse_output;
    /// let p = parse_output("0, 100, 24745, 32607\n---vas-power---\n0, 399.86, 400.00, 575.00, 1695, 3090, 0x0000000000000004\n");
    /// let g = &p.gpus[0];
    /// assert_eq!(g.throttle_words(), Some(vec!["power_cap"]));
    /// assert_eq!((g.power_w, g.power_limit_w, g.power_default_w), (Some(400), Some(400), Some(575)));
    /// assert_eq!((g.sm_mhz, g.sm_max_mhz), (Some(1695), Some(3090)));
    /// ```
    #[must_use]
    pub fn throttle_words(&self) -> Option<Vec<&'static str>> {
        let mask = self.throttle_mask?;
        let mut words: Vec<&'static str> = Vec::new();
        for (bit, word) in THROTTLE_WORDS {
            if mask & bit != 0 && !words.contains(&word) {
                words.push(word);
            }
        }
        Some(words)
    }
}

/// What `--log` found on a box: the newest file matching the pattern, and its last line.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LogReading {
    /// No file on the box matched the pattern.
    NoMatch,
    /// The newest match is not a regular file (a directory, say).
    NotAFile {
        /// Its path on the box, control characters dropped (see `strip`).
        file: String,
    },
    /// The newest matching file has no line to show: it is empty, or its last lines are blank
    /// once cleaned (see `clean`).
    Empty {
        /// The file's path on the box, control characters dropped (see `strip`).
        file: String,
    },
    /// The newest matching file's last non-blank line; with [`LogQuery::grep`], its last line
    /// matching the pattern (empty lines are not searched; a line of spaces only can match, and
    /// is empty once cleaned: the table then says `(a blank line)`).
    Line {
        /// The file's path on the box, control characters dropped (see `strip`).
        file: String,
        /// Its last non-blank (or last matching) line, cleaned (see `clean`).
        line: String,
    },
    /// With [`LogQuery::grep`]: no line of the newest matching file matches the pattern (in the part
    /// searched, its last 64 MiB).
    NoMatchingLine {
        /// The file's path on the box, control characters dropped (see `strip`).
        file: String,
    },
    /// With [`LogQuery::grep`]: the box's `grep` refused the pattern (not an extended regular
    /// expression it knows: `(` alone, say).
    PatternRefused {
        /// The file's path on the box, control characters dropped (see `strip`).
        file: String,
    },
    /// The output after the separator carried none of the remote command's markers (the session
    /// dropped mid-way, say): the probe cannot tell what the box holds.
    Unreadable,
}

/// What `--log` asks of each box: the newest file matching [`LogQuery::path`], and in it the last
/// non-blank line or, with [`LogQuery::grep`], the last line matching a pattern.
///
/// ```
/// use vast_skopos::probe::LogQuery;
/// let q = LogQuery::new("/root/runs/*/train.log").grep("initial loss|Traceback");
/// assert_eq!((q.path.as_str(), q.grep.as_deref()), ("/root/runs/*/train.log", Some("initial loss|Traceback")));
/// assert_eq!(LogQuery::new("/r/a.log").grep, None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LogQuery {
    /// A path or a shell glob, expanded by the box's shell: the user's own pattern, sent to the
    /// user's own box, and left unquoted on purpose.
    pub path: String,
    /// A POSIX extended regular expression (`grep -E`, case-sensitive), quoted for the box's shell
    /// and so never expanded: the last line matching it is shown instead of the last line, searched
    /// in the file's last 64 MiB within 10 s, each carriage-return segment (a progress bar's state)
    /// a line of its own, empty lines left out, each line's first 4096 bytes. `None`: the last
    /// non-blank line.
    pub grep: Option<String>,
}

impl LogQuery {
    /// The newest file matching `path`, and its last non-blank line.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            grep: None,
        }
    }

    /// The last line matching `pattern` instead of the last line (see [`LogQuery::grep`]).
    #[must_use]
    pub fn grep(mut self, pattern: impl Into<String>) -> Self {
        self.grep = Some(pattern.into());
        self
    }
}

/// Which way a probe reached the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Route {
    /// Through vast.ai's `ssh` proxy (the `SSH` column's address).
    #[default]
    Proxy,
    /// Through the box's direct address, past the proxy ([`Instance::direct_endpoint`]), after the
    /// proxy never reached it.
    Direct,
}

/// What one probe found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Probe {
    /// One reading per GPU; empty when `nvidia-smi` printed nothing usable.
    pub gpus: Vec<GpuReading>,
    /// What `--log` found; `None` when `--log` was not given.
    pub log: Option<LogReading>,
    /// Which way the probe reached the box.
    pub route: Route,
}

/// What a look learned of one box over `ssh`.
///
/// `None` when it was not probed (no `--ssh`, or the box is not running), else the probe or the
/// reason it failed. A look holds one per instance, in the instances' order.
pub type ProbeOutcome = Option<Result<Probe>>;

/// The longest one probe may take, connection included; the `ssh` process is killed after it.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// The longest log line shown, in characters; vast.ai's status message under the table is cut to it
/// too (one width for a line of someone else's text that has a line to itself).
pub(crate) const LOG_WIDTH: usize = 120;

/// The line separating `nvidia-smi`'s output from the log's in the remote command's output.
const SEPARATOR: &str = "---vas-log---";

/// The line separating the power query from the throttle query that follows it. The reasons that
/// hold the clocks down are asked on their own, under the name current drivers use and, failing that,
/// the older one (`clocks_event_reasons` and `clocks_throttle_reasons`), so a driver that knows only
/// one of the names costs the throttle reading and not the power and clock ones.
const THROTTLE_SEPARATOR: &str = "---vas-throttle---";

/// The line opening the apps section: each GPU's PCI bus id, then ([`APPS_BY_GPU`]) the memory the
/// compute processes hold on each bus id.
const APPS_SEPARATOR: &str = "---vas-apps---";

/// The line separating the GPUs' bus ids from the compute processes' memory.
const APPS_BY_GPU: &str = "---vas-apps-by-gpu---";

/// The last line of the processes' list when the query succeeded: without it, an empty list could
/// be a failed query, and the memory would be wrongly all "someone else's".
const APPS_OK: &str = "vas-apps-ok";

/// The line opening the power-maximum section (`index,power.max_limit`): asked on its own, after
/// every other section, so a driver that lacks the field costs this reading and nothing else.
const POWER_MAX_SEPARATOR: &str = "---vas-power-max---";

/// The line separating the main `nvidia-smi` query from the power query that follows it. The power
/// query is asked on its own, so a driver that lacks one of its fields costs the power readings
/// and nothing else.
const POWER_SEPARATOR: &str = "---vas-power---";

/// The prefix of the line naming the newest matching file, after [`SEPARATOR`].
const FILE_MARK: &str = "vas-file:";

/// The prefix of the line naming the newest match when it is not a regular file.
const NOT_A_FILE: &str = "vas-not-a-file:";

/// The line saying that no file matched, after [`SEPARATOR`].
const NO_FILE: &str = "vas-no-file";

/// The prefix of the line naming the newest matching file when a pattern is searched in it
/// ([`LogQuery::grep`]): the lines after it are the last match, if any, and [`GREP_EXIT`].
const GREP_FILE_MARK: &str = "vas-grep-file:";

/// The prefix of the last line of a search: `grep`'s own exit status (0 a line matched, 1 none
/// did, 2 the pattern was refused), printed by the box's shell AFTER the match, so no line of the
/// log can stand in for it.
const GREP_EXIT: &str = "vas-grep-exit:";

/// The bytes searched at the end of the file by [`LogQuery::grep`]: 64 MiB, so a log of gigabytes
/// is not read whole (an older match is not seen).
const GREP_BYTES: usize = 64 * 1024 * 1024;

/// The longest the search of [`LogQuery::grep`] may take on the box, in seconds: the whole of it
/// (`tail`, `tr`, `cut` and `grep`) runs under one `timeout`, and a search cut by it reads as
/// [`LogReading::Unreadable`], never as an answer about an older part of the file.
const GREP_SECS: u64 = 10;

/// The lines read from the end of the file: the last non-blank one among them is shown.
const TAIL_LINES: usize = 20;

/// The bytes read from the end of the file, before its lines are counted: a progress bar rewritten
/// in place for hours (one enormous line, with no newline to stop a line count) cannot fill the pipe
/// or run past the box's 5 s `timeout` (which would print nothing, and read as an empty file); the
/// tail of the bytes holds the bar's final state.
const TAIL_BYTES: usize = 4096;

/// Parses `nvidia-smi --query-gpu=index,utilization.gpu,memory.used,memory.total
/// --format=csv,noheader,nounits`; lines that do not parse are skipped.
///
/// ```
/// use vast_skopos::probe::parse_nvidia_smi;
/// let g = parse_nvidia_smi("0, 98, 30122, 32607\n1, [N/A], 0, 32607\n");
/// assert_eq!(g.len(), 1);
/// assert_eq!((g[0].index, g[0].util_pct, g[0].mem_used_mib), (0, 98, 30122));
/// ```
#[must_use]
pub fn parse_nvidia_smi(csv: &str) -> Vec<GpuReading> {
    csv.lines()
        .filter_map(|line| {
            let mut f = line.split(',').map(str::trim);
            Some(GpuReading {
                index: f.next()?.parse().ok()?,
                util_pct: f.next()?.parse().ok()?,
                mem_used_mib: f.next()?.parse().ok()?,
                mem_total_mib: f.next()?.parse().ok()?,
                other_mem_mib: None,
                power_w: None,
                power_limit_w: None,
                power_default_w: None,
                sm_mhz: None,
                sm_max_mhz: None,
                throttle_mask: None,
                power_max_w: None,
                processes: None,
            })
        })
        .collect()
}

/// `text` as one word of a POSIX shell, quoted so that nothing in it is expanded: `'...'`, each `'`
/// written `'\''`.
fn shell_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The command run on the box.
///
/// `nvidia-smi`'s query, then, with `log`, the separator and the NEWEST match of its path (a path
/// or a shell glob, expanded on the box): a `vas-file:` line and the file's last lines (at most 20,
/// capped at 4096 bytes), a `vas-not-a-file:` line when the match is not a regular file, or a
/// `vas-no-file` line when nothing matched. With [`LogQuery::grep`], a `vas-grep-file:` line, the last
/// line of the file's last 64 MiB that matches (`grep -a -E`, each carriage-return segment a line,
/// empty lines left out, each line's first 4096 bytes), and `vas-grep-exit:` with the status of the
/// search (`grep`'s; 3 when the file cannot be read; `timeout`'s 124 when the search ran past 10 s).
///
/// ```
/// use vast_skopos::probe::{LogQuery, remote_command};
/// assert!(remote_command(None).starts_with("timeout 20 nvidia-smi --query-gpu="));
/// let cmd = remote_command(Some(&LogQuery::new("/root/run/*.log")));
/// assert!(cmd.contains("ls -dt -- /root/run/*.log"));
/// assert!(cmd.contains("printf 'vas-file:%s\\n' \"$f\""));
/// assert!(cmd.contains("tail -c 4096 -- \"$f\" | tail -n 20"));
/// assert!(cmd.contains("printf 'vas-not-a-file:%s\\n' \"$f\"") && cmd.contains("echo 'vas-no-file'"));
/// // The pattern is quoted: the box's shell expands nothing in it.
/// let cmd = remote_command(Some(&LogQuery::new("/r/*.log").grep("it's $HOME")));
/// assert!(cmd.contains("timeout 10 sh -c '"), "the whole search under one deadline");
/// assert!(cmd.contains("echo \"vas-grep-exit:$?\""));
/// ```
#[must_use]
pub fn remote_command(log: Option<&LogQuery>) -> String {
    // `timeout` on the box guards `nvidia-smi` and `tail`; the glob's expansion and `ls` are bounded
    // only by the local deadline ([`PROBE_TIMEOUT`]), which kills `ssh`.
    let smi = format!(
        "timeout 20 nvidia-smi --query-gpu=index,utilization.gpu,memory.used,memory.total \
         --format=csv,noheader,nounits; echo '{POWER_SEPARATOR}'; \
         timeout 20 nvidia-smi --query-gpu=index,power.draw,power.limit,power.default_limit,\
         clocks.sm,clocks.max.sm --format=csv,noheader,nounits 2>/dev/null; \
         echo '{THROTTLE_SEPARATOR}'; \
         timeout 20 nvidia-smi --query-gpu=index,clocks_event_reasons.active \
         --format=csv,noheader,nounits 2>/dev/null || \
         timeout 20 nvidia-smi --query-gpu=index,clocks_throttle_reasons.active \
         --format=csv,noheader,nounits 2>/dev/null; echo '{APPS_SEPARATOR}'; \
         timeout 20 nvidia-smi --query-gpu=index,pci.bus_id --format=csv,noheader,nounits 2>/dev/null; \
         echo '{APPS_BY_GPU}'; \
         timeout 20 nvidia-smi --query-compute-apps=gpu_bus_id,used_gpu_memory \
         --format=csv,noheader,nounits 2>/dev/null && echo '{APPS_OK}'; \
         echo '{POWER_MAX_SEPARATOR}'; \
         timeout 20 nvidia-smi --query-gpu=index,power.max_limit --format=csv,noheader,nounits 2>/dev/null"
    );
    // The glob is left unquoted on purpose so the box's shell expands it: the pattern is the
    // user's own, sent to the user's own box. The `grep` pattern is the other value of the user's
    // interpolated here, and it is quoted ([`shell_quoted`]): nothing in it is expanded. Everything
    // else is a constant.
    log.map_or_else(
        || smi.clone(),
        |query| {
            let pattern = &query.path;
            // The last line, or the last line matching. The search runs under ONE `timeout` (a
            // `sh -c` of its own, the file passed as `$1`), so `tr` and `grep` are bounded too and a
            // search cut short says so (124), instead of answering about the part it read; a file
            // that cannot be read (gone since `ls`, say) says so too (3), instead of "no line
            // matches". `tr` makes each carriage-return segment a line (a progress bar's matching
            // state is found); the empty lines are dropped (a `\r\n` would leave one after every
            // line), so an empty line is never searched; `cut` keeps each line's head, where a log
            // line says what it is, so a matching line of megabytes cannot fill the pipe; `-a` keeps
            // one NUL byte from turning the answer into "Binary file matches"; `-e` keeps a pattern
            // starting with `-` a pattern; `tail`'s and `grep`'s complaints go nowhere (the status
            // says them), so they cannot stand in for `ssh`'s reason; `$?` is the search's status.
            let read = query.grep.as_deref().map_or_else(
                || {
                    format!(
                        "printf '{FILE_MARK}%s\\n' \"$f\"; \
                         timeout 5 tail -c {TAIL_BYTES} -- \"$f\" | tail -n {TAIL_LINES}"
                    )
                },
                |grep| {
                    let search = shell_quoted(&format!(
                        "[ -r \"$1\" ] || exit 3; tail -c {GREP_BYTES} -- \"$1\" 2>/dev/null \
                         | tr '\\r' '\\n' | grep -a -v -e '^$' | cut -b 1-{TAIL_BYTES} \
                         | grep -a -E -e {} 2>/dev/null",
                        shell_quoted(grep)
                    ));
                    let kept = TAIL_BYTES + 128;
                    format!(
                        "printf '{GREP_FILE_MARK}%s\\n' \"$f\"; \
                         {{ timeout {GREP_SECS} sh -c {search} vas-grep \"$f\"; \
                         echo \"{GREP_EXIT}$?\"; }} | tail -n 2 | tail -c {kept}"
                    )
                },
            );
            // The match is named on its own line before the file's last lines, so the table can
            // say which run wrote it, and "nothing matched", "not a file" and "empty" stay apart.
            // `ls -d` lists a matched directory itself (not its contents); `--` keeps a match
            // starting with `-` from reading as an option; `printf`, not `echo`, so a backslash in
            // a path is not expanded (`dash`'s `echo` does).
            format!(
                "{smi}; echo '{SEPARATOR}'; f=$(ls -dt -- {pattern} 2>/dev/null | head -n 1); \
                 if [ -z \"$f\" ]; then echo '{NO_FILE}'; \
                 elif [ -f \"$f\" ]; then {read}; \
                 else printf '{NOT_A_FILE}%s\\n' \"$f\"; fi"
            )
        },
    )
}

/// Remote text made safe for the table, uncut.
///
/// Its last non-blank carriage-return segment is kept (a progress bar's final state, even when
/// the bar ends on a `\r`), and control characters (terminal escapes included) are dropped. Paths
/// go through this alone: [`clean`]'s width cut would keep a long path's head and lose its run and
/// file name.
///
/// ```
/// use vast_skopos::probe::strip;
/// assert_eq!(strip("10%\r55%\r100% done"), "100% done");
/// assert_eq!(strip("10%\r55%\r"), "55%");
/// assert_eq!(strip("\u{1b}[31mred\u{1b}[0m"), "[31mred[0m");
/// assert_eq!(strip(&"x".repeat(200)).chars().count(), 200);
/// assert_eq!(strip("\r \r"), "");
/// ```
#[doc(hidden)]
#[must_use]
pub fn strip(text: &str) -> String {
    text.rsplit('\r')
        .map(|segment| {
            let kept: String = segment.chars().filter(|&c| !is_hidden(c)).collect();
            // BORROW: explicit to_owned -- the trimmed segment is returned owned
            kept.trim().to_owned()
        })
        .find(|segment| !segment.is_empty())
        .unwrap_or_default()
}

/// A remote log line made safe for the table: [`strip`], then its first 120 characters (an
/// ellipsis after them marks a cut).
///
/// ```
/// use vast_skopos::probe::clean;
/// assert_eq!(clean("10%\r55%\r100% done"), "100% done");
/// assert_eq!(clean(&"x".repeat(200)).chars().count(), 121);
/// ```
#[doc(hidden)]
#[must_use]
pub fn clean(line: &str) -> String {
    let kept = strip(line);
    shortened(&kept, LOG_WIDTH).unwrap_or(kept)
}

/// Splits the remote output into the probe; `log` is `None` when the output has no separator
/// (`--log` was not given).
///
/// ```
/// use vast_skopos::probe::{parse_output, LogReading};
/// let p = parse_output("0, 97, 100, 200\n---vas-log---\nvas-file:/r/s0/train.log\nepoch 41\n");
/// assert_eq!(p.gpus.len(), 1);
/// assert_eq!(p.log, Some(LogReading::Line {
///     file: "/r/s0/train.log".to_owned(), line: "epoch 41".to_owned() }));
/// assert_eq!(parse_output("0, 97, 100, 200\n").log, None);
/// ```
#[must_use]
pub fn parse_output(stdout: &str) -> Probe {
    let (gpus_and_power, log) = match stdout.split_once(SEPARATOR) {
        Some((smi, log)) => (smi, Some(parse_log(log))),
        None => (stdout, None),
    };
    let (gpus_and_power, power_max) = gpus_and_power
        .split_once(POWER_MAX_SEPARATOR)
        .unwrap_or((gpus_and_power, ""));
    let (smi, rest) = gpus_and_power
        .split_once(POWER_SEPARATOR)
        .unwrap_or((gpus_and_power, ""));
    let (power_and_throttle, apps) = rest.split_once(APPS_SEPARATOR).unwrap_or((rest, ""));
    let (power, throttle) = power_and_throttle
        .split_once(THROTTLE_SEPARATOR)
        .unwrap_or((power_and_throttle, ""));
    let mut gpus = parse_nvidia_smi(smi);
    merge_power(&mut gpus, power);
    merge_throttle(&mut gpus, throttle);
    merge_apps(&mut gpus, apps);
    merge_power_max(&mut gpus, power_max);
    Probe {
        gpus,
        log,
        route: Route::Proxy,
    }
}

/// The PCI bus id of GPU `index` in the apps section's first half (`0, 00000000:C1:00.0`), in lower
/// case so the two halves compare whatever case the driver prints.
fn bus_of(buses: &str, index: u32) -> Option<String> {
    buses.lines().find_map(|line| {
        let (i, bus) = line.split_once(',')?;
        (i.trim().parse::<u32>().ok()? == index).then(|| bus.trim().to_lowercase())
    })
}

/// Reads the apps section into each GPU's [`GpuReading::processes`] (how many processes are listed
/// on its bus id) and [`GpuReading::other_mem_mib`]: the device's memory in use minus the sum of
/// what the compute processes hold on its bus id. Nothing is concluded unless the processes' query
/// ended with its success line and the GPU's bus id is known; and no `other_mem_mib` unless at
/// least one process is listed on it, and every figure of that GPU's processes parses (`[N/A]` is
/// not zero).
fn merge_apps(gpus: &mut [GpuReading], apps: &str) {
    let Some((buses, by_gpu)) = apps.split_once(APPS_BY_GPU) else {
        return;
    };
    if !by_gpu.lines().any(|line| line.trim() == APPS_OK) {
        return;
    }
    for gpu in gpus {
        let Some(bus) = bus_of(buses, gpu.index) else {
            continue;
        };
        let mut accounted: u64 = 0;
        let mut complete = true;
        let mut listed: u32 = 0;
        for line in by_gpu.lines() {
            let Some((on, memory)) = line.split_once(',') else {
                continue;
            };
            if on.trim().to_lowercase() != bus {
                continue;
            }
            listed = listed.saturating_add(1);
            match memory.trim().parse::<u64>() {
                Ok(mib) => accounted = accounted.saturating_add(mib),
                Err(_) => complete = false,
            }
        }
        // A GPU with NO process listed is unknown, not "everyone else's": a run that has just
        // restarted, or a box whose container cannot see its own processes, lists none while the
        // memory is still its own.
        gpu.processes = Some(listed);
        gpu.other_mem_mib =
            (complete && listed > 0).then(|| gpu.mem_used_mib.saturating_sub(accounted));
    }
}

/// A number of watts or MHz as `nvidia-smi` prints it (`399.86`), rounded; `None` for `[N/A]`,
/// text, a negative or a non-finite number.
fn rounded_u32(text: &str) -> Option<u32> {
    let value = text.trim().parse::<f64>().ok()?;
    // CAST: f64 → u32, a rounded non-negative reading in watts or MHz (checked above), saturating
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    let rounded = value.round() as u32;
    (value.is_finite() && value >= 0.0).then_some(rounded)
}

/// A clock event reasons mask as `nvidia-smi` prints it (`0x0000000000000004`): hexadecimal, with a
/// `0x` or `0X` prefix (one, never two); `None` for anything else.
fn parse_mask(text: &str) -> Option<u64> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u64::from_str_radix(digits, 16).ok()
}

/// Reads the throttle query (`index,clocks_event_reasons.active`) into the GPUs it names; lines that
/// do not parse (an error message `nvidia-smi` printed for a field its driver lacks, say) or name a
/// GPU the main query did not are skipped.
fn merge_throttle(gpus: &mut [GpuReading], throttle: &str) {
    for line in throttle.lines() {
        let Some((index, mask)) = line.split_once(',') else {
            continue;
        };
        let (Ok(index), Some(mask)) = (index.trim().parse::<u32>(), parse_mask(mask.trim())) else {
            continue;
        };
        if let Some(gpu) = gpus.iter_mut().find(|g| g.index == index) {
            gpu.throttle_mask = Some(mask);
        }
    }
}

/// Reads the power-maximum query (`index,power.max_limit`) into the GPUs it names; lines that do not
/// parse (an error `nvidia-smi` printed for a field its driver lacks, say) or name a GPU the main
/// query did not are skipped.
fn merge_power_max(gpus: &mut [GpuReading], power_max: &str) {
    for line in power_max.lines() {
        let Some((index, watts)) = line.split_once(',') else {
            continue;
        };
        let (Ok(index), Some(watts)) = (index.trim().parse::<u32>(), rounded_u32(watts)) else {
            continue;
        };
        if let Some(gpu) = gpus.iter_mut().find(|g| g.index == index) {
            gpu.power_max_w = Some(watts);
        }
    }
}

/// Reads the power query (`index,power.draw,power.limit,power.default_limit,clocks.sm,
/// clocks.max.sm`) into the GPUs it names; lines that do not parse, or name a GPU the main query
/// did not, are skipped, and a field the box did not give stays `None`. A seventh field, the
/// throttle mask, is read too when a line carries it (one query with every field says the same).
fn merge_power(gpus: &mut [GpuReading], power: &str) {
    for line in power.lines() {
        let mut f = line.split(',').map(str::trim);
        let Some(index) = f.next().and_then(|i| i.parse::<u32>().ok()) else {
            continue;
        };
        let Some(gpu) = gpus.iter_mut().find(|g| g.index == index) else {
            continue;
        };
        gpu.power_w = f.next().and_then(rounded_u32);
        gpu.power_limit_w = f.next().and_then(rounded_u32);
        gpu.power_default_w = f.next().and_then(rounded_u32);
        gpu.sm_mhz = f.next().and_then(rounded_u32);
        gpu.sm_max_mhz = f.next().and_then(rounded_u32);
        if let Some(mask) = f.next().and_then(parse_mask) {
            gpu.throttle_mask = Some(mask);
        }
    }
}

/// Reads the log half of the remote output, what follows the `---vas-log---` separator.
///
/// Its first non-blank line decides: `vas-no-file`, `vas-not-a-file:<path>`,
/// `vas-file:<path>` followed by the file's last lines, of which the last non-blank one is shown, or
/// `vas-grep-file:<path>` followed by the last match and `vas-grep-exit:<grep's status>` (0: the
/// line, 1: [`LogReading::NoMatchingLine`], 2: [`LogReading::PatternRefused`]). Anything else is
/// [`LogReading::Unreadable`]. A log line cannot forge a state: only the line right after the
/// separator is read as a marker, and the status of a search is its LAST line.
///
/// ```
/// use vast_skopos::probe::{parse_log, LogReading};
/// assert_eq!(parse_log("\nvas-no-file\n"), LogReading::NoMatch);
/// assert_eq!(parse_log("\nvas-not-a-file:/r/s0\n"),
///     LogReading::NotAFile { file: "/r/s0".to_owned() });
/// assert_eq!(parse_log("\nvas-file:/r/a.log\n"),
///     LogReading::Empty { file: "/r/a.log".to_owned() });
/// assert_eq!(parse_log("\nvas-file:/r/a.log\n  \u{1b}\n"),
///     LogReading::Empty { file: "/r/a.log".to_owned() });
/// assert_eq!(parse_log("\nvas-file:/r/a.log\r\n10%\r100% done\r\n\n  \n"),
///     LogReading::Line { file: "/r/a.log".to_owned(), line: "100% done".to_owned() });
/// assert_eq!(parse_log("\nvas-file:/r/a.log\nepoch 1\nvas-no-file\n"),
///     LogReading::Line { file: "/r/a.log".to_owned(), line: "vas-no-file".to_owned() });
/// let long = format!("/{}/run/train.log", "d".repeat(150));
/// assert_eq!(parse_log(&format!("\nvas-file:{long}\nx\n")),
///     LogReading::Line { file: long, line: "x".to_owned() });
/// // A search: the match, then grep's status; a forged status line is only a match.
/// let found = "\nvas-grep-file:/r/a.log\nvas-grep-exit:1\nvas-grep-exit:0\n";
/// assert_eq!(parse_log(found),
///     LogReading::Line { file: "/r/a.log".to_owned(), line: "vas-grep-exit:1".to_owned() });
/// assert_eq!(parse_log("\nvas-grep-file:/r/a.log\nvas-grep-exit:1\n"),
///     LogReading::NoMatchingLine { file: "/r/a.log".to_owned() });
/// assert_eq!(parse_log("\nvas-grep-file:/r/a.log\nvas-grep-exit:2\n"),
///     LogReading::PatternRefused { file: "/r/a.log".to_owned() });
/// assert_eq!(parse_log("\nvas-grep-file:/r/a.log\ninitial loss 4.2\n"), LogReading::Unreadable,
///     "no status: the answer was cut short");
/// assert_eq!(parse_log("\nvas-grep-file:/r/a.log\n\nvas-grep-exit:0\n"),
///     LogReading::Line { file: "/r/a.log".to_owned(), line: String::new() }, "a blank line matched");
/// assert_eq!(parse_log("\nvas-grep-file:/r/a.log\nvas-grep-exit:124\n"), LogReading::Unreadable,
///     "the search ran past its deadline");
/// assert_eq!(parse_log(""), LogReading::Unreadable);
/// assert_eq!(parse_log("\nconnection reset\n"), LogReading::Unreadable);
/// ```
#[doc(hidden)]
#[must_use]
pub fn parse_log(text: &str) -> LogReading {
    let mut lines = text.lines().skip_while(|l| l.trim().is_empty());
    let Some(first) = lines.next().map(str::trim_end) else {
        return LogReading::Unreadable;
    };
    if first == NO_FILE {
        return LogReading::NoMatch;
    }
    if let Some(path) = first.strip_prefix(NOT_A_FILE) {
        return LogReading::NotAFile { file: strip(path) };
    }
    if let Some(path) = first.strip_prefix(GREP_FILE_MARK) {
        return parse_grep(strip(path), lines);
    }
    let Some(path) = first.strip_prefix(FILE_MARK) else {
        return LogReading::Unreadable;
    };
    let file = strip(path);
    // A forward scan: `skip_while` is not double-ended, so `rfind` is not available here.
    match lines.map(clean).filter(|l| !l.is_empty()).last() {
        Some(line) => LogReading::Line { file, line },
        None => LogReading::Empty { file },
    }
}

/// Reads what follows a `vas-grep-file:` line: the last non-blank line must be `vas-grep-exit:N`,
/// printed by the box's shell after the search was done (a log line cannot stand in for it, since
/// it always comes last); with a status of 0, the line right before it is the match (a line of
/// spaces only is empty once cleaned). A status of 3 (the file could not be read), 124 (the search
/// ran past its deadline), or any other, is [`LogReading::Unreadable`].
fn parse_grep<'a>(file: String, lines: impl Iterator<Item = &'a str>) -> LogReading {
    let mut kept: Vec<&str> = lines.collect();
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    let mut from_end = kept.iter().rev();
    let status = from_end
        .next()
        .and_then(|l| l.trim().strip_prefix(GREP_EXIT))
        .and_then(|code| code.parse::<u8>().ok());
    match status {
        Some(0) => from_end
            .next()
            .map_or(LogReading::Unreadable, |line| LogReading::Line {
                file,
                line: clean(line),
            }),
        Some(1) => LogReading::NoMatchingLine { file },
        Some(2) => LogReading::PatternRefused { file },
        // No status line (the session dropped), or one `grep` never gives.
        _ => LogReading::Unreadable,
    }
}

/// How long the pipes' readers may still deliver once the child has exited: what has arrived by
/// then is kept, the rest is not waited for (a stray process holding a pipe must not hang `vas`).
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Reads `pipe` on its own thread, sending each chunk as soon as it is read, so text already read
/// survives a pipe that some stray process keeps open. `Err` when the thread cannot be started.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::io::Result<mpsc::Receiver<Vec<u8>>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name(String::from("vas-pipe"))
        .spawn(move || {
            let Some(mut p) = pipe else { return };
            let mut buf = [0_u8; 8192];
            // EXPLICIT: an imperative read loop: each chunk is sent the moment it is read.
            loop {
                match p.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = buf.get(..n).unwrap_or_default().to_vec();
                        if tx.send(chunk).is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        })?;
    Ok(rx)
}

/// The text a drained pipe delivered by `deadline`, decoded leniently: one invalid UTF-8 byte (a
/// latin-1 file name, a cut character) must not erase the whole reading.
fn collect(rx: &mpsc::Receiver<Vec<u8>>, deadline: Instant) -> String {
    let mut bytes = Vec::new();
    // EXPLICIT: a stateful accumulation, until the writer closes the pipe or the deadline passes.
    while let Ok(chunk) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        bytes.extend(chunk);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The `ssh` command for one probe, before its pipes are set: every option and every value is its
/// own argument, never a shell string, so a path with a space stays one path and nothing the box or
/// the user supplied can be read as an option.
///
/// ```
/// use std::ffi::OsStr;
/// use std::path::Path;
/// let cmd = vast_skopos::probe::ssh_command("ssh6.vast.ai", 36000, Some(Path::new("/my keys/id")), None);
/// let args: Vec<&OsStr> = cmd.get_args().collect();
/// assert_eq!(cmd.get_program(), "ssh");
/// assert_eq!(&args[..2], ["-p", "36000"]);
/// let at = args.iter().position(|a| *a == "-i").unwrap();
/// assert_eq!(args[at + 1], "/my keys/id", "one path, spaces and all");
/// let end = args.iter().position(|a| *a == "--").unwrap();
/// assert_eq!(args[end + 1], "root@ssh6.vast.ai", "the target comes after `--`");
/// assert_eq!(args.len(), end + 3, "and the remote command is the last argument");
/// ```
#[doc(hidden)]
#[must_use]
pub fn ssh_command(
    host: &str,
    port: u16,
    identity: Option<&Path>,
    log: Option<&LogQuery>,
) -> Command {
    let mut cmd = Command::new("ssh");
    cmd.args(["-p", &port.to_string()]);
    cmd.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"]);
    cmd.args(["-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=3"]);
    // One session of its own, whatever the user's `~/.ssh/config` says: a `ControlPersist` master
    // would keep the pipes open after the command, and the probe would wait out the drain grace.
    cmd.args(["-o", "ControlMaster=no", "-o", "ControlPath=none"]);
    cmd.args([
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-o",
        "LogLevel=ERROR",
    ]);
    if let Some(file) = identity {
        cmd.arg("-i").arg(file);
    }
    // `--` ends ssh's options, so nothing after it can be read as one.
    cmd.arg("--")
        .arg(format!("root@{host}"))
        .arg(remote_command(log));
    cmd
}

/// The outcome for instance `n`; `None` when `probes` is shorter than the instances (read as "not
/// probed"), like an instance that was not probed.
pub(crate) fn outcome_at(probes: &[ProbeOutcome], n: usize) -> Option<&Result<Probe>> {
    probes.get(n).and_then(Option::as_ref)
}

/// Kills `child` and waits for it, so no process is left behind (the outcome is of no use: the
/// probe has already failed).
fn reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// One box's probe over its two routes, `probe_one` being one attempt at one `host:port`: the proxy
/// once; then, only when the proxy never reached the box ([`SkoposError::is_unreachable`]: a refused
/// key will not change with the route) and the box has a direct address, that address once. A box
/// with no proxy endpoint is probed at its direct address. Never a third attempt; `None` without
/// either endpoint.
fn probe_routes(
    proxy: Option<(&str, u16)>,
    direct: Option<(&str, u16)>,
    mut probe_one: impl FnMut(&str, u16) -> Result<Probe>,
) -> ProbeOutcome {
    let by_direct = |outcome: Result<Probe>| {
        outcome.map(|mut probe| {
            probe.route = Route::Direct;
            probe
        })
    };
    let Some((host, port)) = proxy else {
        let (host, port) = direct?;
        return Some(by_direct(probe_one(host, port)));
    };
    let first = probe_one(host, port);
    let Err(proxy_error) = first else {
        return Some(first);
    };
    let Some((direct_host, direct_port)) = direct.filter(|d| *d != (host, port)) else {
        return Some(Err(proxy_error));
    };
    if !proxy_error.is_unreachable() {
        return Some(Err(proxy_error));
    }
    Some(
        by_direct(probe_one(direct_host, direct_port)).map_err(|direct_error| {
            SkoposError::SshRoutes {
                target: format!("{host}:{port}"),
                message: ssh_reason(&proxy_error),
                direct_target: format!("{direct_host}:{direct_port}"),
                direct_message: ssh_reason(&direct_error),
            }
        }),
    )
}

/// The reason a probe gave (`ssh`'s own words), or the whole error for one that is not a probe's.
fn ssh_reason(error: &SkoposError) -> String {
    if let SkoposError::Ssh { message, .. } = error {
        // BORROW: explicit clone -- the combined error owns its parts
        message.clone()
    } else {
        error.to_string()
    }
}

/// Probes every running box that has an `ssh` endpoint, all at once (one thread each).
///
/// Returns one outcome per instance, in the instances' order: `None` for a box that is not running
/// or has neither endpoint. One attempt per route: the proxy, then, only when it never reached the
/// box, the box's direct address ([`Instance::direct_endpoint`]); see [`Probe::route`] for which way
/// the readings came, and [`SkoposError::SshRoutes`] for a box neither route reached.
#[must_use]
pub fn probe_all(
    instances: &[Instance],
    identity: Option<&Path>,
    log: Option<&LogQuery>,
) -> Vec<ProbeOutcome> {
    std::thread::scope(|scope| {
        // EXPLICIT: collected so every probe is SPAWNED before any is joined; a lazy chain would
        // run them one after another.
        #[allow(clippy::needless_collect)]
        let handles: Vec<_> = instances
            .iter()
            .map(|i| {
                scope.spawn(move || {
                    if !i.is_running() {
                        return None;
                    }
                    probe_routes(i.ssh_endpoint(), i.direct_endpoint(), |host, port| {
                        ssh_probe(host, port, identity, log)
                    })
                })
            })
            .collect();
        handles
            .into_iter()
            .zip(instances)
            .map(|(handle, i)| {
                handle.join().unwrap_or_else(|_| {
                    let target = i
                        .ssh_endpoint()
                        .or_else(|| i.direct_endpoint())
                        .map_or_else(|| String::from("?"), |(h, p)| format!("{h}:{p}"));
                    Some(Err(SkoposError::ssh(target, "the probe thread panicked")))
                })
            })
            .collect()
    })
}

/// The reason of a probe that `ssh` never answered within [`PROBE_TIMEOUT`]: one that
/// [`SkoposError::is_unreachable`] reads as unreachable.
fn no_answer() -> String {
    format!(
        "no answer within {} s (ssh killed)",
        PROBE_TIMEOUT.as_secs()
    )
}

/// The reason of a probe the box began to answer (its stdout came back) but that did not finish
/// within [`PROBE_TIMEOUT`]: a hung `nvidia-smi`, say. The box WAS reached, and
/// [`SkoposError::is_unreachable`] does not read this as unreachable.
fn unfinished() -> String {
    format!(
        "the box answered, but the probe did not finish within {} s (ssh killed)",
        PROBE_TIMEOUT.as_secs()
    )
}

/// Probes `root@host:port` once.
///
/// # Errors
/// Returns [`SkoposError::Ssh`] if the host name starts with `-`, if `ssh` cannot be started, if it
/// gives no answer within [`PROBE_TIMEOUT`] (or the box began to answer and did not finish within
/// it), or if it prints no GPU reading (refused, host key changed, ...).
pub fn ssh_probe(
    host: &str,
    port: u16,
    identity: Option<&Path>,
    log: Option<&LogQuery>,
) -> Result<Probe> {
    let target = format!("{host}:{port}");
    // Defence in depth: the target is passed as `root@host` after `--`, so a host starting with `-`
    // could not read as an option, and a name like that is a malformed one all the same.
    if host.starts_with('-') {
        return Err(SkoposError::ssh(
            target,
            "host name starts with '-' (refused: not a host name)",
        ));
    }
    let mut cmd = ssh_command(host, port, identity, log);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| SkoposError::ssh(target.clone(), format!("failed to run ssh: {e}")))?;
    // Both pipes are drained on their own threads WHILE the child runs: a pipe buffer is 4 KB on
    // Windows, and a child writing more than that into an undrained pipe blocks until killed.
    let (out, err) = match (drain(child.stdout.take()), drain(child.stderr.take())) {
        (Ok(out), Ok(err)) => (out, err),
        (Err(e), _) | (_, Err(e)) => {
            reap(&mut child);
            return Err(SkoposError::ssh(
                target,
                format!("failed to read ssh's output: {e}"),
            ));
        }
    };
    let started = Instant::now();
    // EXPLICIT: a deadline on ONE child process, polled; it never starts a second attempt.
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= PROBE_TIMEOUT => {
                reap(&mut child);
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => {
                reap(&mut child);
                return Err(SkoposError::ssh(
                    target,
                    format!("failed to wait for ssh: {e}"),
                ));
            }
        }
    };
    let Some(status) = status else {
        // Only the remote command writes to `ssh`'s stdout: any of it says the box was reached.
        let answered = !collect(&out, Instant::now() + DRAIN_GRACE)
            .trim()
            .is_empty();
        let reason = if answered { unfinished() } else { no_answer() };
        return Err(SkoposError::ssh(target, reason));
    };
    // One deadline for both pipes, so a stray holder costs at most `DRAIN_GRACE` in all.
    let deadline = Instant::now() + DRAIN_GRACE;
    let (stdout, stderr) = (collect(&out, deadline), collect(&err, deadline));
    let mut probe = parse_output(&stdout);
    if log.is_some() && probe.log.is_none() {
        // `--log` was given but the separator never came back: say "cannot tell", not "-".
        probe.log = Some(LogReading::Unreadable);
    }
    if probe.gpus.is_empty() {
        let reason = stderr
            .lines()
            .map(clean)
            .rfind(|l| !l.is_empty())
            .unwrap_or_else(|| {
                format!("no GPU reading (ssh exit {})", status.code().unwrap_or(-1))
            });
        return Err(SkoposError::ssh(target, reason));
    }
    Ok(probe)
}

#[cfg(test)]
// EXPLICIT: test code -- a failed unwrap or panic IS the failed assertion; and the fake probers
// return the `Result` that `probe_routes` takes.
#[allow(clippy::unwrap_used, clippy::panic, clippy::unnecessary_wraps)]
mod tests {
    use super::{Probe, Route, no_answer, probe_routes, unfinished};
    use crate::error::{Result, SkoposError};

    #[test]
    fn a_probe_ssh_never_answered_reads_as_unreachable_and_one_cut_short_does_not() {
        assert!(SkoposError::ssh("h:1", no_answer()).is_unreachable());
        assert!(!SkoposError::ssh("h:1", unfinished()).is_unreachable());
    }

    const PROXY: (&str, u16) = ("ssh6.vast.ai", 10_732);
    const DIRECT: (&str, u16) = ("79.117.26.24", 20_674);

    /// [`probe_routes`] with a fake one-attempt prober answering `answer(host, port)`; the outcome
    /// and every attempt made, in order.
    fn routes(
        proxy: Option<(&'static str, u16)>,
        direct: Option<(&'static str, u16)>,
        answer: impl Fn(&str, u16) -> Result<Probe>,
    ) -> (Option<Result<Probe>>, Vec<(String, u16)>) {
        let mut tried = Vec::new();
        let outcome = probe_routes(proxy, direct, |host, port| {
            tried.push((host.to_owned(), port));
            answer(host, port)
        });
        (outcome, tried)
    }

    fn refused(host: &str, port: u16) -> Result<Probe> {
        Err(SkoposError::ssh(
            format!("{host}:{port}"),
            format!("ssh: connect to host {host} port {port}: Connection refused"),
        ))
    }

    fn answered(_: &str, _: u16) -> Result<Probe> {
        Ok(Probe::default())
    }

    fn tried(attempts: &[(&str, u16)]) -> Vec<(String, u16)> {
        attempts.iter().map(|&(h, p)| (h.to_owned(), p)).collect()
    }

    #[test]
    fn a_proxy_that_answers_is_the_only_attempt() {
        let (outcome, attempts) = routes(Some(PROXY), Some(DIRECT), answered);
        assert_eq!(outcome.unwrap().unwrap().route, Route::Proxy);
        assert_eq!(attempts, tried(&[PROXY]));
    }

    #[test]
    fn a_proxy_that_never_reached_the_box_is_followed_by_one_direct_attempt() {
        let direct_only = |host: &str, port: u16| {
            if (host, port) == DIRECT {
                Ok(Probe::default())
            } else {
                refused(host, port)
            }
        };
        let (outcome, attempts) = routes(Some(PROXY), Some(DIRECT), direct_only);
        assert_eq!(outcome.unwrap().unwrap().route, Route::Direct);
        assert_eq!(attempts, tried(&[PROXY, DIRECT]));
        // Both refuse: one attempt each, and both reasons.
        let (outcome, attempts) = routes(Some(PROXY), Some(DIRECT), refused);
        assert_eq!(attempts, tried(&[PROXY, DIRECT]), "never a third attempt");
        let error = outcome.unwrap().unwrap_err();
        assert!(error.is_unreachable(), "{error}");
        let SkoposError::SshRoutes {
            target,
            direct_target,
            direct_message,
            ..
        } = &error
        else {
            panic!("both routes: {error:?}");
        };
        assert_eq!(
            (target.as_str(), direct_target.as_str()),
            ("ssh6.vast.ai:10732", "79.117.26.24:20674")
        );
        assert!(
            direct_message.contains("Connection refused"),
            "{direct_message}"
        );
    }

    #[test]
    fn a_box_the_proxy_reached_is_not_tried_again_by_another_route() {
        let key_refused = |host: &str, port: u16| {
            Err(SkoposError::ssh(
                format!("{host}:{port}"),
                "root@ssh6.vast.ai: Permission denied (publickey).",
            ))
        };
        let (outcome, attempts) = routes(Some(PROXY), Some(DIRECT), key_refused);
        assert_eq!(
            attempts,
            tried(&[PROXY]),
            "a refused key is refused by any route"
        );
        assert!(matches!(outcome, Some(Err(SkoposError::Ssh { .. }))));
    }

    #[test]
    fn without_a_direct_address_or_without_a_proxy_one_route_is_tried() {
        let (outcome, attempts) = routes(Some(PROXY), None, refused);
        assert_eq!(attempts, tried(&[PROXY]));
        assert!(matches!(outcome, Some(Err(SkoposError::Ssh { .. }))));
        let (outcome, attempts) = routes(None, Some(DIRECT), answered);
        assert_eq!(attempts, tried(&[DIRECT]));
        assert_eq!(outcome.unwrap().unwrap().route, Route::Direct);
        let (outcome, attempts) = routes(None, None, answered);
        assert!(outcome.is_none() && attempts.is_empty());
        // A direct address equal to the proxy's is no second route.
        let (_, attempts) = routes(Some(PROXY), Some(PROXY), refused);
        assert_eq!(attempts, tried(&[PROXY]));
    }
}

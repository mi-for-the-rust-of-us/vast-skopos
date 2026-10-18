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
use crate::instance::{Instance, shortened};

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
    /// software power cap); `None` when the box did not say.
    pub throttle_mask: Option<u64>,
}

/// The clock event reasons that mean the card is held below what it could do, as `nvidia-smi`
/// names their bits: the software power cap, the thermal slowdowns, the hardware slowdown and the
/// power brake. An idle GPU, application clocks and sync boost are not in the list: they are not a
/// restraint on a run.
const THROTTLE_WORDS: [(u64, &str); 5] = [
    (0x4, "power_cap"),
    (0x8, "slowdown"),
    (0x20, "thermal"),
    (0x40, "thermal"),
    (0x80, "power_brake"),
];

impl GpuReading {
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
    /// The newest matching file's last non-blank line.
    Line {
        /// The file's path on the box, control characters dropped (see `strip`).
        file: String,
        /// Its last non-blank line, cleaned (see `clean`).
        line: String,
    },
    /// The output after the separator carried none of the remote command's markers (the session
    /// dropped mid-way, say): the probe cannot tell what the box holds.
    Unreadable,
}

/// What one probe found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Probe {
    /// One reading per GPU; empty when `nvidia-smi` printed nothing usable.
    pub gpus: Vec<GpuReading>,
    /// What `--log` found; `None` when `--log` was not given.
    pub log: Option<LogReading>,
}

/// What a look learned of one box over `ssh`.
///
/// `None` when it was not probed (no `--ssh`, or the box is not running), else the probe or the
/// reason it failed. A look holds one per instance, in the instances' order.
pub type ProbeOutcome = Option<Result<Probe>>;

/// The longest one probe may take, connection included; the `ssh` process is killed after it.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// The longest log line shown, in characters.
const LOG_WIDTH: usize = 120;

/// The line separating `nvidia-smi`'s output from the log's in the remote command's output.
const SEPARATOR: &str = "---vas-log---";

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

/// The lines read from the end of the file: the last non-blank one among them is shown.
const TAIL_LINES: usize = 20;

/// The bytes those lines are capped at on the box, so a progress bar rewritten in place for hours
/// (one enormous line) cannot fill the pipe; the tail of the bytes holds the bar's final state.
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
                power_w: None,
                power_limit_w: None,
                power_default_w: None,
                sm_mhz: None,
                sm_max_mhz: None,
                throttle_mask: None,
            })
        })
        .collect()
}

/// The command run on the box.
///
/// `nvidia-smi`'s query, then, with `log`, the separator and the NEWEST match of `log` (a path or
/// a shell glob, expanded on the box): a `vas-file:` line and the file's last lines (at most 20,
/// capped at 4096 bytes), a `vas-not-a-file:` line when the match is not a regular file, or a
/// `vas-no-file` line when nothing matched.
///
/// ```
/// use vast_skopos::probe::remote_command;
/// assert!(remote_command(None).starts_with("timeout 20 nvidia-smi --query-gpu="));
/// let cmd = remote_command(Some("/root/run/*.log"));
/// assert!(cmd.contains("ls -dt -- /root/run/*.log"));
/// assert!(cmd.contains("printf 'vas-file:%s\\n' \"$f\""));
/// assert!(cmd.contains("tail -n 20 -- \"$f\" | tail -c 4096"));
/// assert!(cmd.contains("printf 'vas-not-a-file:%s\\n' \"$f\"") && cmd.contains("echo 'vas-no-file'"));
/// ```
#[must_use]
pub fn remote_command(log: Option<&str>) -> String {
    // `timeout` on the box guards `nvidia-smi` and `tail`; the glob's expansion and `ls` are bounded
    // only by the local deadline ([`PROBE_TIMEOUT`]), which kills `ssh`.
    let smi = format!(
        "timeout 20 nvidia-smi --query-gpu=index,utilization.gpu,memory.used,memory.total \
         --format=csv,noheader,nounits; echo '{POWER_SEPARATOR}'; \
         timeout 20 nvidia-smi --query-gpu=index,power.draw,power.limit,power.default_limit,\
         clocks.sm,clocks.max.sm,clocks_throttle_reasons.active \
         --format=csv,noheader,nounits 2>/dev/null"
    );
    // The glob is left unquoted on purpose so the box's shell expands it: the pattern is the
    // user's own, sent to the user's own box. Everything else interpolated here is a constant.
    log.map_or_else(
        || smi.clone(),
        |pattern| {
            // The match is named on its own line before the file's last lines, so the table can
            // say which run wrote it, and "nothing matched", "not a file" and "empty" stay apart.
            // `ls -d` lists a matched directory itself (not its contents); `--` keeps a match
            // starting with `-` from reading as an option; `printf`, not `echo`, so a backslash in
            // a path is not expanded (`dash`'s `echo` does).
            format!(
                "{smi}; echo '{SEPARATOR}'; f=$(ls -dt -- {pattern} 2>/dev/null | head -n 1); \
                 if [ -z \"$f\" ]; then echo '{NO_FILE}'; \
                 elif [ -f \"$f\" ]; then printf '{FILE_MARK}%s\\n' \"$f\"; \
                 timeout 5 tail -n {TAIL_LINES} -- \"$f\" | tail -c {TAIL_BYTES}; \
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
            let kept: String = segment.chars().filter(|c| !c.is_control()).collect();
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
    let (smi, power) = gpus_and_power
        .split_once(POWER_SEPARATOR)
        .unwrap_or((gpus_and_power, ""));
    let mut gpus = parse_nvidia_smi(smi);
    merge_power(&mut gpus, power);
    Probe { gpus, log }
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

/// Reads the power query (`index,power.draw,power.limit,power.default_limit,clocks.sm,
/// clocks.max.sm,clocks_throttle_reasons.active`) into the GPUs it names; lines that do not parse,
/// or name a GPU the main query did not, are skipped, and a field the box did not give stays `None`.
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
        gpu.throttle_mask = f
            .next()
            .and_then(|mask| u64::from_str_radix(mask.trim_start_matches("0x"), 16).ok());
    }
}

/// Reads the log half of the remote output, what follows the `---vas-log---` separator.
///
/// Its first non-blank line decides: `vas-no-file`, `vas-not-a-file:<path>`, or
/// `vas-file:<path>` followed by the file's last lines, of which the last non-blank one is shown.
/// Anything else is [`LogReading::Unreadable`]. A log line cannot forge a state: only the line
/// right after the separator is read as a marker.
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
pub fn ssh_command(host: &str, port: u16, identity: Option<&Path>, log: Option<&str>) -> Command {
    let mut cmd = Command::new("ssh");
    cmd.args(["-p", &port.to_string()]);
    cmd.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"]);
    cmd.args(["-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=3"]);
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

/// Probes every running box that has an `ssh` endpoint, all at once (one thread each).
///
/// Returns one outcome per instance, in the instances' order: `None` for a box that is not running
/// or has no endpoint. One attempt per box ([`ssh_probe`]).
#[must_use]
pub fn probe_all(
    instances: &[Instance],
    identity: Option<&Path>,
    log: Option<&str>,
) -> Vec<ProbeOutcome> {
    std::thread::scope(|scope| {
        // EXPLICIT: collected so every probe is SPAWNED before any is joined; a lazy chain would
        // run them one after another.
        #[allow(clippy::needless_collect)]
        let handles: Vec<_> = instances
            .iter()
            .map(|i| {
                scope.spawn(move || {
                    let (host, port) = i.ssh_endpoint().filter(|_| i.is_running())?;
                    Some(ssh_probe(host, port, identity, log))
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
                        .map_or_else(|| String::from("?"), |(h, p)| format!("{h}:{p}"));
                    Some(Err(SkoposError::ssh(target, "the probe thread panicked")))
                })
            })
            .collect()
    })
}

/// Probes `root@host:port` once.
///
/// # Errors
/// Returns [`SkoposError::Ssh`] if the host name starts with `-`, if `ssh` cannot be started, if it
/// gives no answer within [`PROBE_TIMEOUT`], or if it prints no GPU reading (refused, host key
/// changed, ...).
pub fn ssh_probe(
    host: &str,
    port: u16,
    identity: Option<&Path>,
    log: Option<&str>,
) -> Result<Probe> {
    let target = format!("{host}:{port}");
    if host.starts_with('-') {
        return Err(SkoposError::ssh(
            target,
            "host name starts with '-' (refused: it would read as an option)",
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
        return Err(SkoposError::ssh(
            target,
            format!(
                "no answer within {} s (ssh killed)",
                PROBE_TIMEOUT.as_secs()
            ),
        ));
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

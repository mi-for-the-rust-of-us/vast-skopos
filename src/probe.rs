//! Asking a box directly, over the system `ssh`: what are its GPUs doing, and what did the run
//! last write?
//!
//! The system `ssh` is used rather than an `ssh` library, so the user's keys, agent and
//! `~/.ssh/config` apply unchanged. ONE attempt per call: a box that refuses is reported as such
//! and never retried in a loop (`BatchMode=yes`, `ConnectTimeout=10`), which is a person's
//! decision to make, from the dashboard.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{Result, SkoposError};

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
}

/// What one probe found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Probe {
    /// One reading per GPU; empty when `nvidia-smi` printed nothing usable.
    pub gpus: Vec<GpuReading>,
    /// The last line of the log, when `--log` was given and the file exists.
    pub last_log: Option<String>,
}

/// The longest one probe may take, connection included; the `ssh` process is killed after it.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// The longest log line shown, in characters.
const LOG_WIDTH: usize = 120;

/// The line separating `nvidia-smi`'s output from the log's in the remote command's output.
const SEPARATOR: &str = "---vas-log---";

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
            })
        })
        .collect()
}

/// The remote command: `nvidia-smi`'s query, then, with `log`, the separator and the last line of
/// the NEWEST file matching `log` (a path or a shell glob, expanded on the box).
///
/// ```
/// use vast_skopos::probe::remote_command;
/// assert!(remote_command(None).starts_with("timeout 20 nvidia-smi --query-gpu="));
/// assert!(remote_command(Some("/root/run/*.log")).contains("ls -t /root/run/*.log"));
/// ```
#[must_use]
pub fn remote_command(log: Option<&str>) -> String {
    // `timeout` on the box too: a wedged `nvidia-smi` or a dead mount under the glob must not
    // hold the session open until the local deadline.
    let smi = "timeout 20 nvidia-smi --query-gpu=index,utilization.gpu,memory.used,memory.total \
               --format=csv,noheader,nounits";
    // The glob is left unquoted on purpose so the box's shell expands it: the pattern is the
    // user's own, sent to the user's own box.
    log.map_or_else(
        // BORROW: explicit to_owned -- the command is returned owned
        || smi.to_owned(),
        |pattern| {
            format!(
                "{smi}; echo '{SEPARATOR}'; f=$(ls -t {pattern} 2>/dev/null | head -n 1); \
                 [ -n \"$f\" ] && timeout 5 tail -n 1 \"$f\""
            )
        },
    )
}

/// A remote log line made safe for the table: what follows its last carriage return (a progress
/// bar's final state), control characters (terminal escapes included) dropped, at most
/// 120 characters (an ellipsis marks a cut).
///
/// ```
/// use vast_skopos::probe::clean;
/// assert_eq!(clean("10%\r55%\r100% done"), "100% done");
/// assert_eq!(clean("\u{1b}[31mred\u{1b}[0m"), "[31mred[0m");
/// assert_eq!(clean(&"x".repeat(200)).chars().count(), 121);
/// ```
#[must_use]
pub fn clean(line: &str) -> String {
    let last = line.rsplit('\r').next().unwrap_or(line);
    let kept: String = last.chars().filter(|c| !c.is_control()).collect();
    let kept = kept.trim();
    if kept.chars().count() > LOG_WIDTH {
        let mut cut: String = kept.chars().take(LOG_WIDTH).collect();
        cut.push('…');
        cut
    } else {
        // BORROW: explicit to_owned -- the cleaned line is returned owned
        kept.to_owned()
    }
}

/// Splits the remote output into the probe.
///
/// ```
/// use vast_skopos::probe::parse_output;
/// let p = parse_output("0, 97, 100, 200\n---vas-log---\nepoch 41 loss 0.123\n");
/// assert_eq!(p.gpus.len(), 1);
/// assert_eq!(p.last_log.as_deref(), Some("epoch 41 loss 0.123"));
/// ```
#[must_use]
pub fn parse_output(stdout: &str) -> Probe {
    let (smi, log) = stdout.split_once(SEPARATOR).unwrap_or((stdout, ""));
    let last_log = log.lines().map(clean).rfind(|l| !l.is_empty());
    Probe {
        gpus: parse_nvidia_smi(smi),
        last_log,
    }
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
        return Err(SkoposError::Ssh {
            target,
            message: String::from(
                "host name starts with '-' (refused: it would read as an option)",
            ),
        });
    }
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
        // Passed as its own argument, never through a shell: a path with a space stays one path.
        cmd.arg("-i").arg(file);
    }
    // `--` ends ssh's options, so nothing after it can be read as one.
    cmd.arg("--")
        .arg(format!("root@{host}"))
        .arg(remote_command(log));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| SkoposError::Ssh {
        target: target.clone(),
        message: format!("failed to run ssh: {e}"),
    })?;
    // EXPLICIT: a deadline on ONE child process, polled; it never starts a second attempt. The
    // output is a few lines, far below a pipe buffer, so the child cannot block on a full pipe.
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= PROBE_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => {
                let _ = child.kill();
                return Err(SkoposError::Ssh {
                    target,
                    message: format!("failed to wait for ssh: {e}"),
                });
            }
        }
    };
    let Some(status) = status else {
        return Err(SkoposError::Ssh {
            target,
            message: format!(
                "no answer within {} s (ssh killed)",
                PROBE_TIMEOUT.as_secs()
            ),
        });
    };
    let (mut stdout, mut stderr) = (String::new(), String::new());
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_string(&mut stdout);
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    let probe = parse_output(&stdout);
    if probe.gpus.is_empty() {
        let reason = stderr
            .lines()
            .map(clean)
            .rfind(|l| !l.is_empty())
            .unwrap_or_else(|| {
                format!("no GPU reading (ssh exit {})", status.code().unwrap_or(-1))
            });
        return Err(SkoposError::Ssh {
            target,
            message: reason,
        });
    }
    Ok(probe)
}

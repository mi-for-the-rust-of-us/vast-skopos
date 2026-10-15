//! Asking a box directly, over the system `ssh`: what are its GPUs doing, and what did the run
//! last write?
//!
//! The system `ssh` is used rather than an `ssh` library, so the user's keys, agent and
//! `~/.ssh/config` apply unchanged. ONE attempt per call: a box that refuses is reported as such
//! and never retried in a loop (`BatchMode=yes`, `ConnectTimeout=10`), which is a person's
//! decision to make, from the dashboard.

use std::path::Path;
use std::process::Command;

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
/// assert!(remote_command(None).starts_with("nvidia-smi --query-gpu="));
/// assert!(remote_command(Some("/root/run/*.log")).contains("ls -t /root/run/*.log"));
/// ```
#[must_use]
pub fn remote_command(log: Option<&str>) -> String {
    let smi = "nvidia-smi --query-gpu=index,utilization.gpu,memory.used,memory.total \
             --format=csv,noheader,nounits";
    // The glob is left unquoted on purpose so the box's shell expands it: the pattern is the
    // user's own, sent to the user's own box.
    log.map_or_else(
        // BORROW: explicit to_owned -- the command is returned owned
        || smi.to_owned(),
        |pattern| {
            format!(
                "{smi}; echo '{SEPARATOR}'; f=$(ls -t {pattern} 2>/dev/null | head -n 1);                  [ -n \"$f\" ] && tail -n 1 \"$f\""
            )
        },
    )
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
    let last_log = log
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(str::to_owned);
    Probe {
        gpus: parse_nvidia_smi(smi),
        last_log,
    }
}

/// Probes `root@host:port` once.
///
/// # Errors
/// Returns [`SkoposError::Ssh`] if `ssh` cannot be started, exits non-zero (refused, timed out,
/// host key changed, ...) or prints no GPU reading.
pub fn ssh_probe(
    host: &str,
    port: u16,
    identity: Option<&Path>,
    log: Option<&str>,
) -> Result<Probe> {
    let target = format!("{host}:{port}");
    let mut cmd = Command::new("ssh");
    cmd.args(["-p", &port.to_string()]);
    cmd.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"]);
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
    cmd.arg(format!("root@{host}")).arg(remote_command(log));
    let out = cmd.output().map_err(|e| SkoposError::Ssh {
        target: target.clone(),
        message: format!("failed to run ssh: {e}"),
    })?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let probe = parse_output(&stdout);
    if probe.gpus.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let reason = stderr
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty())
            .map_or_else(
                || {
                    format!(
                        "no GPU reading (ssh exit {})",
                        out.status.code().unwrap_or(-1)
                    )
                },
                str::to_owned,
            );
        return Err(SkoposError::Ssh {
            target,
            message: reason,
        });
    }
    Ok(probe)
}

//! What the API says about an instance and the account, read leniently.
//!
//! The rows are read field by field from `serde_json::Value` rather than through a derived
//! deserializer: the API adds, drops and retypes fields over time, and a monitor that stops
//! working because one unused field changed type is worse than one that shows a dash.

use serde_json::Value;

/// One rented instance, as the table needs it.
///
/// Every field but `id` is optional: a field the API did not send (or sent with another type) is
/// `None`, and the table shows a dash.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Instance {
    /// The instance id.
    pub id: u64,
    /// The user's label, when one was set.
    pub label: Option<String>,
    /// The GPU model (e.g. `RTX 5090`).
    pub gpu_name: Option<String>,
    /// How many GPUs.
    pub num_gpus: Option<u64>,
    /// What the instance is doing (`running`, `loading`, `exited`, ...).
    pub actual_status: Option<String>,
    /// What it was asked to do; differs from `actual_status` while it changes.
    pub intended_status: Option<String>,
    /// The host's last status message, when any.
    pub status_msg: Option<String>,
    /// The total price, in dollars per hour (GPU, storage and bandwidth included by vast.ai).
    pub dph_total: Option<f64>,
    /// When the instance was created, in Unix seconds.
    pub start_date: Option<f64>,
    /// The `ssh` proxy host (e.g. `ssh4.vast.ai`).
    pub ssh_host: Option<String>,
    /// The `ssh_port` field as the API gives it. NOT always the port to connect to: see
    /// [`Instance::ssh_endpoint`].
    pub ssh_port: Option<u16>,
    /// How the instance was launched (e.g. `ssh`, `jupyter`); it decides the proxy port.
    pub image_runtype: Option<String>,
    /// The host's location, when given (e.g. `Vietnam, VN`).
    pub geolocation: Option<String>,
    /// The machine's id: what `vas offers --exclude-machine` takes, and what a rentals log keeps.
    pub machine_id: Option<u64>,
    /// The host's id: what `vas offers --exclude-host` takes.
    pub host_id: Option<u64>,
    /// The CPU's model, as the host reports it (e.g. `AMD EPYC 7K62 48-Core Processor`): with the
    /// driver, what tells one machine of a host from another.
    pub cpu_name: Option<String>,
    /// The NVIDIA driver's version (e.g. `580.142`).
    pub driver_version: Option<String>,
    /// vast.ai's own benchmark score of the machine, `DLPerf` (an RTX 5090 reads about 199). It did NOT
    /// show a host's 500 W power cap on two machines (2026-10-25): the card's own reading does
    /// (`vas --ssh`, `POWER`).
    pub dlperf: Option<f64>,
    /// The bandwidth between the host and the GPU that vast.ai measured, in GB/s.
    pub pcie_bw: Option<f64>,
    /// The box's public address (`public_ipaddr`): with [`Instance::direct_ssh_port`], its direct route,
    /// past vast.ai's `ssh` proxy. A host behind a router that does not forward the port has one that
    /// refuses (seen 2026-10-26).
    pub public_ipaddr: Option<String>,
    /// The host port mapped to the box's port 22 (the row's `ports["22/tcp"]`), for the direct route.
    pub direct_ssh_port: Option<u16>,
}

/// Whether `c` must not reach a terminal from someone else's text: a control character, or an
/// invisible one that can still reorder or hide what is shown (the soft hyphen and the combining
/// grapheme joiner, the Arabic letter mark, zero-width characters, the line and paragraph
/// separators, bidirectional embeddings, overrides and isolates, word joiners and the deprecated
/// format characters, variation selectors, the byte-order mark, interlinear annotations, and the
/// tag characters that can spell a hidden message).
///
/// Everything text from a box, a host or the API shows goes through this: the table cells, the
/// JSON, the probe's log lines and file names, and `ssh`'s error line.
pub(crate) fn is_hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{034F}'
                | '\u{061C}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FEFF}'
                | '\u{FFF9}'..='\u{FFFB}'
                | '\u{E0000}'..='\u{E007F}'
                | '\u{E0100}'..='\u{E01EF}'
        )
}

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

/// The first `max` characters of `text` and an ellipsis, when `text` has more; `None` when it fits.
pub(crate) fn shortened(text: &str, max: usize) -> Option<String> {
    (text.chars().count() > max).then(|| {
        let mut cut: String = text.chars().take(max).collect();
        cut.push('…');
        cut
    })
}

/// Whether `c` breaks a line or a cell: a line feed, a carriage return, a tab, a vertical tab, a form
/// feed, the next-line character (`U+0085`), or the line and paragraph separators. In text shown on
/// one line it becomes a space.
const fn is_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\t' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// A string field, as one line: a line break or a tab becomes one space ([`is_break`]; a run of them,
/// and the spaces around it, make a single space), and every other hidden character is dropped
/// ([`is_hidden`]: the text comes from the API and from other people's hosts, and never reaches the
/// terminal raw); `None` when absent, null, empty or not a string.
///
/// vast.ai's status message holds the lines of an image's download: dropping the breaks glued them
/// (`Verifying Checksumc455daa85bb6:`, seen 2026-10-28).
pub(crate) fn text(row: &Value, field: &str) -> Option<String> {
    let mut kept = String::new();
    // After a break, the spaces around it are held back: the next visible character gets one.
    let mut broken = false;
    // EXPLICIT: an imperative loop: whether a space goes in depends on what came before.
    for c in row.get(field).and_then(Value::as_str)?.chars() {
        if is_break(c) {
            kept.truncate(kept.trim_end_matches(' ').len());
            broken = true;
        } else if !is_hidden(c) && (!broken || c != ' ') {
            if broken && !kept.is_empty() {
                kept.push(' ');
            }
            broken = false;
            kept.push(c);
        }
    }
    let kept = kept.trim();
    // BORROW: explicit to_owned -- the row outlives the response it was read from
    (!kept.is_empty()).then(|| kept.to_owned())
}

/// A number field, accepting a JSON number or a numeric string; `None` when not finite (a string
/// can spell `NaN` or `inf`).
pub(crate) fn number(row: &Value, field: &str) -> Option<f64> {
    let n = match row.get(field)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
    }?;
    n.is_finite().then_some(n)
}

/// A boolean field; `None` when absent or not a JSON boolean.
pub(crate) fn boolean(row: &Value, field: &str) -> Option<bool> {
    row.get(field).and_then(Value::as_bool)
}

/// A whole, non-negative number below 2^53 (what an `f64` holds exactly), as an integer: `5.0` is 5.
///
/// ([`whole_secs`] is the crate's one `f64` → `u64` truncation, named for the seconds it was first
/// written for; here the value is already whole.)
fn integral(value: f64) -> Option<u64> {
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value < 9_007_199_254_740_992.0)
        .then(|| whole_secs(value))
}

/// A non-negative integer field, accepting a JSON integer, an integral JSON number written as a
/// float (`123.0`, which some serialisers produce) or an integral numeric string (`"123"`, `"123.0"`).
pub(crate) fn integer(row: &Value, field: &str) -> Option<u64> {
    match row.get(field)? {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().and_then(integral)),
        Value::String(s) => {
            let text = s.trim();
            text.parse()
                .ok()
                .or_else(|| text.parse::<f64>().ok().and_then(integral))
        }
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

impl Instance {
    /// Reads one row of `/api/v1/instances/`; `None` when it has no integer `id`.
    ///
    /// ```
    /// use vast_skopos::instance::Instance;
    /// let row = serde_json::json!({"id": 56053340, "actual_status": "running",
    ///   "dph_total": 0.643, "ssh_port": "23484", "label": ""});
    /// let i = Instance::from_json(&row).unwrap();
    /// assert_eq!((i.id, i.ssh_port, i.label), (56053340, Some(23484), None));
    /// assert!(Instance::from_json(&serde_json::json!({"label": "x"})).is_none());
    /// // A line break becomes a space; other control and invisible characters are dropped; a number
    /// // must be finite.
    /// let lines = serde_json::json!({"id": 3, "status_msg": "c455daa85bb6: Verifying Checksum\r\nc455daa85bb6: Download complete\n"});
    /// assert_eq!(Instance::from_json(&lines).unwrap().status_msg.as_deref(),
    ///   Some("c455daa85bb6: Verifying Checksum c455daa85bb6: Download complete"));
    /// let odd = serde_json::json!({"id": 1, "geolocation": "Tex\u{1b}[2Jas, US", "dph_total": "NaN",
    ///   "gpu_name": "RTX\u{202e} 5090\u{200b}"});
    /// let i = Instance::from_json(&odd).unwrap();
    /// assert_eq!((i.geolocation.as_deref(), i.dph_total), (Some("Tex[2Jas, US"), None));
    /// assert_eq!(i.gpu_name.as_deref(), Some("RTX 5090"));
    /// // The machine and the host, as numbers or numeric strings.
    /// let ids = serde_json::json!({"id": 2, "machine_id": 100682, "host_id": "406325"});
    /// let i = Instance::from_json(&ids).unwrap();
    /// assert_eq!((i.machine_id, i.host_id), (Some(100682), Some(406_325)));
    /// ```
    #[must_use]
    pub fn from_json(row: &Value) -> Option<Self> {
        Some(Self {
            id: integer(row, "id")?,
            label: text(row, "label"),
            gpu_name: text(row, "gpu_name"),
            num_gpus: integer(row, "num_gpus"),
            actual_status: text(row, "actual_status"),
            intended_status: text(row, "intended_status"),
            status_msg: text(row, "status_msg"),
            dph_total: number(row, "dph_total"),
            start_date: number(row, "start_date"),
            ssh_host: text(row, "ssh_host"),
            ssh_port: integer(row, "ssh_port").and_then(|p| u16::try_from(p).ok()),
            image_runtype: text(row, "image_runtype"),
            geolocation: text(row, "geolocation"),
            machine_id: integer(row, "machine_id"),
            host_id: integer(row, "host_id"),
            cpu_name: text(row, "cpu_name"),
            driver_version: text(row, "driver_version"),
            dlperf: number(row, "dlperf"),
            pcie_bw: number(row, "pcie_bw"),
            public_ipaddr: text(row, "public_ipaddr"),
            direct_ssh_port: mapped_ssh_port(row),
        })
    }

    /// Seconds since creation at `now` (Unix seconds); `None` without a start date.
    #[must_use]
    pub fn age_secs(&self, now: f64) -> Option<f64> {
        self.start_date.map(|start| (now - start).max(0.0))
    }

    /// The cost so far at `now`: `dph_total` times the hours since creation.
    ///
    /// An ESTIMATE: it assumes today's price held since creation. vast.ai bills storage while an
    /// instance is stopped and may have changed the price, so the invoice is the reference.
    ///
    /// ```
    /// use vast_skopos::instance::Instance;
    /// let row = serde_json::json!({"id": 1, "dph_total": 0.5, "start_date": 1000.0});
    /// let i = Instance::from_json(&row).unwrap();
    /// assert_eq!(i.cost_so_far(1000.0 + 7200.0), Some(1.0));
    /// ```
    #[must_use]
    pub fn cost_so_far(&self, now: f64) -> Option<f64> {
        Some(self.dph_total? * self.age_secs(now)? / 3600.0)
    }

    /// Whether the instance is running (and so billed at its full `$/h` and worth probing).
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.actual_status.as_deref() == Some("running")
    }

    /// Whether the instance was asked to run (`intended_status` is `running`), whatever it reads now:
    /// a box read `offline` or `loading` that is meant to run may still be billed.
    ///
    /// ```
    /// use vast_skopos::instance::Instance;
    /// let row = |actual: &str, intended: &str| Instance::from_json(&serde_json::json!(
    ///   {"id": 1, "actual_status": actual, "intended_status": intended})).unwrap();
    /// assert!(row("offline", "running").is_meant_to_run());
    /// assert!(!row("running", "stopped").is_meant_to_run(), "stopping: running, but not meant to");
    /// assert!(!Instance::from_json(&serde_json::json!({"id": 1})).unwrap().is_meant_to_run());
    /// ```
    #[must_use]
    pub fn is_meant_to_run(&self) -> bool {
        self.intended_status.as_deref() == Some("running")
    }

    /// The `ssh` proxy endpoint, when the API gave both host and port.
    ///
    /// The official CLI's rule (`_ssh_url`): on the proxy, a Jupyter-launched instance listens on
    /// `ssh_port + 1`, every other one on `ssh_port`. Found by dogfooding (2026-10-16): a box whose
    /// API `ssh_port` read 37240 was reached, by the dashboard's own command, on 37241.
    ///
    /// ```
    /// use vast_skopos::instance::Instance;
    /// let row = |rt: &str| serde_json::json!({"id": 1, "ssh_host": "ssh6.vast.ai",
    ///   "ssh_port": 37240, "image_runtype": rt});
    /// let jupyter = Instance::from_json(&row("jupyter")).unwrap();
    /// assert_eq!(jupyter.ssh_endpoint(), Some(("ssh6.vast.ai", 37241)));
    /// let plain = Instance::from_json(&row("ssh_direc ssh_proxy")).unwrap();
    /// assert_eq!(plain.ssh_endpoint(), Some(("ssh6.vast.ai", 37240)));
    /// ```
    #[must_use]
    pub fn ssh_endpoint(&self) -> Option<(&str, u16)> {
        let jupyter = self
            .image_runtype
            .as_deref()
            .is_some_and(|r| r.contains("jupyter"));
        // Port 0 is no port: a row that says so has no endpoint to probe.
        let port = self
            .ssh_port
            .filter(|p| *p != 0)?
            .checked_add(u16::from(jupyter))?;
        Some((self.ssh_host.as_deref()?, port))
    }

    /// The box's direct `ssh` endpoint, past the proxy: its public address and the host port mapped to
    /// its port 22; `None` without both.
    ///
    /// ```
    /// use vast_skopos::instance::Instance;
    /// let row = serde_json::json!({"id": 1, "public_ipaddr": "180.189.55.38", "ports": {"22/tcp": [
    ///   {"HostIp": "0.0.0.0", "HostPort": "55814"}, {"HostIp": "::", "HostPort": "55814"}]}});
    /// let i = Instance::from_json(&row).unwrap();
    /// assert_eq!(i.direct_endpoint(), Some(("180.189.55.38", 55814)));
    /// assert_eq!(Instance::from_json(&serde_json::json!({"id": 2})).unwrap().direct_endpoint(), None);
    /// ```
    #[must_use]
    pub fn direct_endpoint(&self) -> Option<(&str, u16)> {
        Some((self.public_ipaddr.as_deref()?, self.direct_ssh_port?))
    }
}

/// The host port mapped to the box's port 22, from the row's `ports`
/// (`{"22/tcp": [{"HostIp": "0.0.0.0", "HostPort": "55814"}, {"HostIp": "::", "HostPort": "55814"}]}`, as
/// read on 2026-10-26: the two entries, one per address family, name the same port): the first entry whose
/// `HostPort` is a port above 0.
fn mapped_ssh_port(row: &Value) -> Option<u16> {
    row.get("ports")?
        .get("22/tcp")?
        .as_array()?
        .iter()
        .find_map(|entry| {
            integer(entry, "HostPort")
                .and_then(|port| u16::try_from(port).ok())
                .filter(|port| *port != 0)
        })
}

/// The account's money, from `/api/v0/users/current/`.
///
/// Only these two fields are read: that endpoint's answer also carries the API key, which is
/// never parsed, kept or printed.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Account {
    /// The prepaid credit, in dollars.
    pub credit: Option<f64>,
    /// The balance, in dollars (vast.ai's own field; owed charges make it negative).
    pub balance: Option<f64>,
}

impl Account {
    /// Reads the `/api/v0/users/current/` answer.
    ///
    /// ```
    /// use vast_skopos::instance::Account;
    /// let a = Account::from_json(&serde_json::json!({"credit": 5.94, "balance": 0, "api_key": "x"}));
    /// assert_eq!((a.credit, a.balance), (Some(5.94), Some(0.0)));
    /// ```
    #[must_use]
    pub fn from_json(user: &Value) -> Self {
        Self {
            credit: number(user, "credit"),
            balance: number(user, "balance"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::whole_secs;

    #[test]
    fn whole_seconds_truncate_and_saturate() {
        assert_eq!(whole_secs(59.99), 59);
        assert_eq!((whole_secs(-1.0), whole_secs(f64::NAN)), (0, 0));
        assert_eq!(whole_secs(1e300), u64::MAX);
    }
}

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
    /// The `ssh` proxy port.
    pub ssh_port: Option<u16>,
    /// The host's location, when given (e.g. `Vietnam, VN`).
    pub geolocation: Option<String>,
}

/// A string field, `None` when absent, null, empty or not a string.
fn text(row: &Value, field: &str) -> Option<String> {
    row.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        // BORROW: explicit to_owned -- the instance outlives the response it was read from
        .map(str::to_owned)
}

/// A number field, accepting a JSON number or a numeric string.
fn number(row: &Value, field: &str) -> Option<f64> {
    match row.get(field)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

/// A non-negative integer field, accepting a JSON integer or an integral numeric string.
fn integer(row: &Value, field: &str) -> Option<u64> {
    match row.get(field)? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
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
            geolocation: text(row, "geolocation"),
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

    /// The `ssh` endpoint, when the API gave both host and port.
    #[must_use]
    pub fn ssh_endpoint(&self) -> Option<(&str, u16)> {
        Some((self.ssh_host.as_deref()?, self.ssh_port?))
    }
}

/// The account's money, from `/api/v0/users/current`.
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
    /// Reads the `/api/v0/users/current` answer.
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

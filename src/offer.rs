//! The offers vast.ai lists for rent, read leniently, and the search `vas offers` sends.
//!
//! The search is `GET /api/v0/bundles/?q=<JSON>`, sent WITHOUT any key (the official CLI's
//! `search__offers` now posts the same query; this `GET` form, its commented-out predecessor, is
//! still answered, and keeps `vas` to `GET` alone). Probed 2026-10-16:
//!
//! - the server answers at most [`OFFER_CAP`] rows, whatever `limit` asks, and still says
//!   `truncated: false`; its `order` decides WHICH rows come back;
//! - `allocated_storage` prices that much storage into `dph_total`.
//!
//! So every constraint is sent to the server AND re-checked on each row here ([`OfferQuery::admits`]),
//! and the rows are sorted again here ([`select`]): a row is dropped only when a field it has
//! contradicts a constraint; a missing field keeps the row (the table shows a dash).

use serde_json::{Map, Value, json};

use crate::instance::{boolean, integer, number, text};

/// The rows each search asks for; the server answers at most [`OFFER_CAP`] of them.
pub const FETCH_LIMIT: u32 = 200;

/// The most rows vast.ai answers per search, whatever `limit` asks (probed 2026-10-16, with
/// `truncated: false`): a search answering exactly this many may have been cut.
pub const OFFER_CAP: usize = 64;

/// The storage priced into `$/h` when no disk size is asked, in GB: `vastai search offers`'s own
/// default.
pub const DEFAULT_STORAGE_GB: u32 = 5;

/// One offer, as `vas offers` shows it.
///
/// Every field but `id` is optional: a field the API did not send (or sent with another type) is
/// `None`, and the table shows a dash.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Offer {
    /// The offer id: what `vastai create instance` takes.
    pub id: u64,
    /// The GPU model (e.g. `RTX 5090`).
    pub gpu_name: Option<String>,
    /// How many GPUs.
    pub num_gpus: Option<u64>,
    /// Memory per GPU, in MiB (`nvidia-smi`'s unit: an RTX 5090 reads 32607).
    pub gpu_ram: Option<f64>,
    /// The price, in dollars per hour, with the search's storage included.
    pub dph_total: Option<f64>,
    /// The highest CUDA version the host's driver supports (e.g. `12.8`).
    pub cuda_max_good: Option<f64>,
    /// The disk space available, in GB.
    pub disk_space: Option<f64>,
    /// The host's reliability, 0 to 1 (`reliability2`, else `reliability`).
    pub reliability: Option<f64>,
    /// The upload bandwidth the host advertises, in Mb/s (megabits).
    pub inet_up: Option<f64>,
    /// The download bandwidth the host advertises, in Mb/s.
    pub inet_down: Option<f64>,
    /// The longest rental the host allows, in seconds.
    pub duration: Option<f64>,
    /// The host's location (e.g. `Texas, US`).
    pub geolocation: Option<String>,
    /// The host's id (`--exclude-host`).
    pub host_id: Option<u64>,
    /// The machine's id (`--exclude-machine`); one host may own several machines.
    pub machine_id: Option<u64>,
    /// vast.ai's verdict on the machine (`verified`, `unverified`, `deverified`, ...).
    pub verification: Option<String>,
    /// vast.ai's code for that verdict, undocumented: 0 unverified, 1 verified, 2 and up deverified
    /// (seen 2026-10-25: 2, 4, 6, 8, 10, 12, 14 and 33). What the search filters on.
    pub vericode: Option<u64>,
    /// The CPU's model, as the host reports it (e.g. `AMD EPYC 7K62 48-Core Processor`).
    pub cpu_name: Option<String>,
    /// The NVIDIA driver's version (e.g. `580.142`).
    pub driver_version: Option<String>,
    /// vast.ai's own benchmark score of the machine, `DLPerf` (an RTX 5090 reads about 199). It did NOT
    /// show a host's 500 W power cap on two machines (2026-10-25): the card's own reading does
    /// (`vas --ssh`, `POWER`).
    pub dlperf: Option<f64>,
    /// The bandwidth between the host and the GPU that vast.ai measured, in GB/s.
    pub pcie_bw: Option<f64>,
    /// The GPU's power limit as the listing gives it, in watts (`gpu_max_power`, which the official CLI
    /// calls "GPU power limit (watts)"). It comes from the listing, not from the card: on 2026-10-25 a
    /// machine of host 406325 listed 500 W, the cap `vas --ssh` read on two other machines of that
    /// host, and RTX 5090 offers listed 400 to 575 W. Whether it always shows the enforced limit is not
    /// known; the card's own reading is `vas --ssh`'s `POWER`.
    pub gpu_max_power: Option<f64>,
    /// Whether the offer can be rented.
    pub rentable: Option<bool>,
    /// Whether it is rented already.
    pub rented: Option<bool>,
}

impl Offer {
    /// Reads one row of the search's `offers`; `None` when it has no integer `id`. Text loses its
    /// control and invisible characters (it comes from other people's hosts), and numbers must be
    /// finite.
    ///
    /// ```
    /// use vast_skopos::offer::Offer;
    /// let row = serde_json::json!({"id": 44614424, "gpu_name": "RTX 5090", "dph_total": 0.54,
    ///   "reliability2": 0.991, "reliability": 0.5, "geolocation": "Tex\u{1b}as\u{202e}, US",
    ///   "rentable": true, "rented": false});
    /// let o = Offer::from_json(&row).unwrap();
    /// assert_eq!((o.id, o.reliability, o.geolocation.as_deref()), (44614424, Some(0.991), Some("Texas, US")));
    /// assert_eq!((o.rentable, o.rented), (Some(true), Some(false)));
    /// assert_eq!(Offer::from_json(&serde_json::json!({"id": 1, "reliability": "0.9"})).unwrap().reliability, Some(0.9));
    /// assert!(Offer::from_json(&serde_json::json!({"gpu_name": "RTX 5090"})).is_none());
    /// ```
    #[must_use]
    pub fn from_json(row: &Value) -> Option<Self> {
        Some(Self {
            id: integer(row, "id")?,
            gpu_name: text(row, "gpu_name"),
            num_gpus: integer(row, "num_gpus"),
            gpu_ram: number(row, "gpu_ram"),
            dph_total: number(row, "dph_total"),
            cuda_max_good: number(row, "cuda_max_good"),
            disk_space: number(row, "disk_space"),
            reliability: number(row, "reliability2").or_else(|| number(row, "reliability")),
            inet_up: number(row, "inet_up"),
            inet_down: number(row, "inet_down"),
            duration: number(row, "duration"),
            geolocation: text(row, "geolocation"),
            host_id: integer(row, "host_id"),
            machine_id: integer(row, "machine_id"),
            verification: text(row, "verification"),
            vericode: integer(row, "vericode"),
            cpu_name: text(row, "cpu_name"),
            driver_version: text(row, "driver_version"),
            dlperf: number(row, "dlperf"),
            pcie_bw: number(row, "pcie_bw"),
            gpu_max_power: number(row, "gpu_max_power"),
            rentable: boolean(row, "rentable"),
            rented: boolean(row, "rented"),
        })
    }

    /// `DLPerf` per dollar per hour: [`Offer::dlperf`] over the price shown ([`Offer::dph_total`],
    /// storage included); `None` without both, or for a price that is not above zero.
    ///
    /// Computed here rather than read from vast.ai's `dlperf_per_dphtotal`, so that it matches the
    /// `$/H` column, which prices in the storage the search asked for.
    ///
    /// ```
    /// use vast_skopos::offer::Offer;
    /// let at = |price: f64| Offer::from_json(&serde_json::json!({"id": 1, "dlperf": 199.0,
    ///   "dph_total": price})).unwrap();
    /// assert_eq!(at(0.5).dlperf_per_dollar(), Some(398.0));
    /// assert_eq!(at(0.0).dlperf_per_dollar(), None);
    /// ```
    #[must_use]
    pub fn dlperf_per_dollar(&self) -> Option<f64> {
        let price = self.dph_total.filter(|p| *p > 0.0)?;
        Some(self.dlperf? / price)
    }

    /// The country code: what follows the last comma of the location, when it is two ASCII
    /// letters.
    ///
    /// ```
    /// use vast_skopos::offer::Offer;
    /// let at = |g: &str| Offer::from_json(&serde_json::json!({"id": 1, "geolocation": g})).unwrap();
    /// assert_eq!(at("Texas, US").country(), Some("US"));
    /// assert_eq!(at("KR").country(), Some("KR"));
    /// assert_eq!(at("Nowhere").country(), None);
    /// ```
    #[must_use]
    pub fn country(&self) -> Option<&str> {
        let code = self.geolocation.as_deref()?.rsplit(',').next()?.trim();
        is_country_code(code).then_some(code)
    }
}

/// Whether `code` is a country code: two ASCII letters.
fn is_country_code(code: &str) -> bool {
    code.len() == 2 && code.bytes().all(|b| b.is_ascii_alphabetic())
}

/// The order of the rows: the server is asked for it (it decides which rows come back), and the
/// rows are sorted again here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum OfferSort {
    /// The cheapest first.
    #[default]
    Price,
    /// The most reliable first.
    Reliability,
    /// The fastest upload first.
    Up,
}

impl OfferSort {
    /// The order in words (`cheapest first`, ...).
    ///
    /// ```
    /// use vast_skopos::offer::OfferSort;
    /// assert_eq!(OfferSort::Up.words(), "fastest upload first");
    /// ```
    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            Self::Price => "cheapest first",
            Self::Reliability => "most reliable first",
            Self::Up => "fastest upload first",
        }
    }

    /// The server's field and direction for this order.
    const fn server_order(self) -> (&'static str, &'static str) {
        match self {
            Self::Price => ("dph_total", "asc"),
            Self::Reliability => ("reliability2", "desc"),
            Self::Up => ("inet_up", "desc"),
        }
    }
}

/// What a search asks for. Its `Default` is the IDENTITY: no constraint, so every offer rentable
/// now (rentable, not rented, on-demand).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct OfferQuery {
    /// The GPU model, exactly as vast.ai names it (e.g. `RTX 5090`).
    pub gpu: Option<String>,
    /// The number of GPUs.
    pub num_gpus: Option<u64>,
    /// The disk wanted, in GB: priced into `$/h` (`allocated_storage`) and required free.
    pub disk: Option<u32>,
    /// The lowest acceptable CUDA version (e.g. `12.8`).
    pub cuda: Option<f64>,
    /// The lowest acceptable reliability, 0 to 1.
    pub reliability: Option<f64>,
    /// The lowest acceptable advertised upload, in Mb/s.
    pub inet_up: Option<f64>,
    /// The lowest acceptable advertised download, in Mb/s.
    pub inet_down: Option<f64>,
    /// The highest acceptable price, in dollars per hour (storage included).
    pub max_price: Option<f64>,
    /// The shortest acceptable maximum rental, in days.
    pub min_days: Option<f64>,
    /// Only machines vast.ai has verified (which hides the unverified ones as well as the deverified).
    pub verified: bool,
    /// Leave out the machines vast.ai has deverified (verified once, then found failing its checks)
    /// and keep the unverified ones (new, or under evaluation: "not a judgment of quality", in vast.ai's
    /// words), which [`OfferQuery::verified`] hides too.
    pub exclude_deverified: bool,
    /// Country codes excluded (e.g. `CN`).
    pub exclude_countries: Vec<String>,
    /// Only these hosts (empty: every host): what a host rented from before offers now.
    pub only_hosts: Vec<u64>,
    /// Only these machines (empty: every machine): is the machine rented before for rent again?
    pub only_machines: Vec<u64>,
    /// Host ids excluded.
    pub exclude_hosts: Vec<u64>,
    /// Machine ids excluded (a host may own several machines, one of which can be the bad one).
    pub exclude_machines: Vec<u64>,
}

/// `x` with at most two decimals, trailing zeros dropped (`99`, `99.5`; `57`, not
/// `56.99999999999999`).
fn two_decimals(x: f64) -> String {
    let fixed = format!("{x:.2}");
    // BORROW: explicit to_owned -- the trimmed text outlives the formatted buffer
    fixed.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Whether `value` passes a lower bound: no bound, or no value (kept, shown as a dash), or at least
/// the bound.
fn at_least(value: Option<f64>, bound: Option<f64>) -> bool {
    bound.is_none_or(|b| value.is_none_or(|v| v >= b))
}

/// `ids`, sorted, each once.
fn sorted_once(ids: &[u64]) -> Vec<u64> {
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    sorted
}

impl OfferQuery {
    /// Whether the query constrains nothing (the identity).
    ///
    /// ```
    /// use vast_skopos::offer::OfferQuery;
    /// assert!(OfferQuery::default().is_identity());
    /// let mut q = OfferQuery::default();
    /// q.exclude_hosts.push(362355);
    /// assert!(!q.is_identity());
    /// let mut q = OfferQuery::default();
    /// q.exclude_machines.push(142875);
    /// assert!(!q.is_identity(), "a machine excluded alone is a constraint");
    /// ```
    #[must_use]
    pub const fn is_identity(&self) -> bool {
        self.gpu.is_none()
            && self.num_gpus.is_none()
            && self.disk.is_none()
            && self.cuda.is_none()
            && self.reliability.is_none()
            && self.inet_up.is_none()
            && self.inet_down.is_none()
            && self.max_price.is_none()
            && self.min_days.is_none()
            && !self.verified
            && !self.exclude_deverified
            && self.exclude_countries.is_empty()
            && self.only_hosts.is_empty()
            && self.only_machines.is_empty()
            && self.exclude_hosts.is_empty()
            && self.exclude_machines.is_empty()
    }

    /// The storage priced into `$/h`, in GB: the disk asked, else [`DEFAULT_STORAGE_GB`].
    ///
    /// ```
    /// use vast_skopos::offer::OfferQuery;
    /// let mut q = OfferQuery::default();
    /// assert_eq!(q.storage_gb(), 5);
    /// q.disk = Some(50);
    /// assert_eq!(q.storage_gb(), 50);
    /// ```
    #[must_use]
    pub fn storage_gb(&self) -> u32 {
        self.disk.unwrap_or(DEFAULT_STORAGE_GB)
    }

    /// The excluded countries, upper-cased, sorted, each once: as sent and as summarised.
    fn countries(&self) -> Vec<String> {
        let mut codes: Vec<String> = self
            .exclude_countries
            .iter()
            .map(|c| c.trim().to_ascii_uppercase())
            .collect();
        codes.sort();
        codes.dedup();
        codes
    }

    /// The ids of the field (`host_id`, `machine_id`) the search keeps (`only`) and leaves out
    /// (`excluded`), as the server's one object for it: `{"in": [...], "notin": [...]}`, each sorted,
    /// each once, each only when given (probed 2026-10-25: the server applies both; one id in both
    /// lists leaves no row); `None` when neither is given.
    fn ids_filter(only: &[u64], excluded: &[u64]) -> Option<Value> {
        let mut ops = Map::new();
        if !only.is_empty() {
            ops.insert("in".into(), json!(sorted_once(only)));
        }
        if !excluded.is_empty() {
            ops.insert("notin".into(), json!(sorted_once(excluded)));
        }
        (!ops.is_empty()).then_some(Value::Object(ops))
    }

    /// The search's `q`, for `sort`: always rentable, not rented, on-demand, [`FETCH_LIMIT`] rows,
    /// the storage, the order; then one key per constraint.
    ///
    /// ```
    /// use vast_skopos::offer::{OfferQuery, OfferSort};
    /// let identity = OfferQuery::default().to_search_json(OfferSort::Price);
    /// assert_eq!(identity, serde_json::json!({"rentable": {"eq": true}, "rented": {"eq": false},
    ///   "type": "on-demand", "limit": 200, "allocated_storage": 5,
    ///   "order": [["dph_total", "asc"]]}));
    /// let mut q = OfferQuery::default();
    /// q.gpu = Some("RTX 5090".to_owned());
    /// q.disk = Some(50);
    /// q.min_days = Some(2.0);
    /// q.exclude_countries = vec!["US".to_owned(), "CN".to_owned(), "CN".to_owned()];
    /// q.exclude_machines = vec![142875, 100682, 142875];
    /// let v = q.to_search_json(OfferSort::Up);
    /// assert_eq!(v["gpu_name"], serde_json::json!({"eq": "RTX 5090"}));
    /// assert_eq!((v["allocated_storage"].as_u64(), v["disk_space"]["gte"].as_u64()), (Some(50), Some(50)));
    /// assert_eq!(v["duration"]["gte"].as_f64(), Some(172_800.0));
    /// assert_eq!(v["geolocation"], serde_json::json!({"notin": ["CN", "US"]}));
    /// assert_eq!(v["machine_id"], serde_json::json!({"notin": [100682, 142875]}));
    /// assert_eq!(v["order"], serde_json::json!([["inet_up", "desc"]]));
    /// ```
    #[must_use]
    pub fn to_search_json(&self, sort: OfferSort) -> Value {
        let mut q = Map::new();
        q.insert("rentable".into(), json!({"eq": true}));
        q.insert("rented".into(), json!({"eq": false}));
        q.insert("type".into(), json!("on-demand"));
        q.insert("limit".into(), json!(FETCH_LIMIT));
        q.insert("allocated_storage".into(), json!(self.storage_gb()));
        let (field, direction) = sort.server_order();
        q.insert("order".into(), json!([[field, direction]]));
        if let Some(g) = &self.gpu {
            q.insert("gpu_name".into(), json!({"eq": g}));
        }
        if let Some(n) = self.num_gpus {
            q.insert("num_gpus".into(), json!({"eq": n}));
        }
        if let Some(d) = self.disk {
            q.insert("disk_space".into(), json!({"gte": d}));
        }
        let lower = [
            ("cuda_max_good", self.cuda),
            ("reliability2", self.reliability),
            ("inet_up", self.inet_up),
            ("inet_down", self.inet_down),
            ("duration", self.min_days.map(|d| d * 86_400.0)),
        ];
        for (field, bound) in lower {
            if let Some(b) = bound {
                q.insert(field.into(), json!({"gte": b}));
            }
        }
        if let Some(p) = self.max_price {
            q.insert("dph_total".into(), json!({"lte": p}));
        }
        if self.verified {
            q.insert("verified".into(), json!({"eq": true}));
        }
        // The server does not filter on `verification` (probed 2026-10-25: `neq`, `in` and `notin` all
        // came back with deverified rows); it does on `vericode`: 0 unverified, 1 verified, 2 and up
        // deverified. Not sent beside `verified`, which already leaves the deverified out: given both,
        // the server applied only one (an unverified row came back to `verified`).
        if self.exclude_deverified && !self.verified {
            q.insert("vericode".into(), json!({"lte": 1}));
        }
        if !self.exclude_countries.is_empty() {
            q.insert("geolocation".into(), json!({"notin": self.countries()}));
        }
        if let Some(hosts) = Self::ids_filter(&self.only_hosts, &self.exclude_hosts) {
            q.insert("host_id".into(), hosts);
        }
        if let Some(machines) = Self::ids_filter(&self.only_machines, &self.exclude_machines) {
            q.insert("machine_id".into(), machines);
        }
        Value::Object(q)
    }

    /// Whether `offer` passes every constraint, re-checked here (the identity's too: rentable, not
    /// rented): a field the offer has that contradicts a constraint drops it; a missing field
    /// keeps it. The GPU model is compared without regard to ASCII case.
    ///
    /// ```
    /// use vast_skopos::offer::{Offer, OfferQuery};
    /// let offer = |row| Offer::from_json(&row).unwrap();
    /// let mut q = OfferQuery::default();
    /// q.reliability = Some(0.99);
    /// q.exclude_countries = vec!["CN".to_owned()];
    /// q.verified = true;
    /// assert!(q.admits(&offer(serde_json::json!({"id": 1, "reliability2": 0.995, "geolocation": "Texas, US"}))));
    /// assert!(!q.admits(&offer(serde_json::json!({"id": 2, "reliability2": 0.51}))));
    /// assert!(!q.admits(&offer(serde_json::json!({"id": 3, "geolocation": "Guangdong, CN"}))));
    /// assert!(!q.admits(&offer(serde_json::json!({"id": 4, "verification": "deverified"}))));
    /// assert!(!q.admits(&offer(serde_json::json!({"id": 5, "rented": true}))));
    /// assert!(q.admits(&offer(serde_json::json!({"id": 6}))), "missing fields keep the row");
    /// ```
    #[must_use]
    pub fn admits(&self, offer: &Offer) -> bool {
        let same_text = |want: Option<&String>, have: Option<&String>| {
            want.is_none_or(|w| have.is_none_or(|h| h.eq_ignore_ascii_case(w)))
        };
        let available = offer.rentable != Some(false) && offer.rented != Some(true);
        let same_count =
            |want: Option<u64>, have: Option<u64>| want.is_none_or(|w| have.is_none_or(|h| h == w));
        let disk = self.disk.map(f64::from);
        let min_secs = self.min_days.map(|d| d * 86_400.0);
        let price_ok = self
            .max_price
            .is_none_or(|p| offer.dph_total.is_none_or(|v| v <= p));
        let verified_ok = !self.verified
            || offer
                .verification
                .as_deref()
                .is_none_or(|v| v == "verified");
        // The same rule as the server's (`vericode <= 1`), and the text it goes with.
        let deverified_ok = !self.exclude_deverified
            || (offer.verification.as_deref() != Some("deverified")
                && offer.vericode.is_none_or(|code| code <= 1));
        let country_ok = offer.country().is_none_or(|c| {
            !self
                .exclude_countries
                .iter()
                .any(|x| x.trim().eq_ignore_ascii_case(c))
        });
        let wanted = |id: Option<u64>, only: &[u64], excluded: &[u64]| {
            id.is_none_or(|i| (only.is_empty() || only.contains(&i)) && !excluded.contains(&i))
        };
        let host_ok = wanted(offer.host_id, &self.only_hosts, &self.exclude_hosts);
        let machine_ok = wanted(
            offer.machine_id,
            &self.only_machines,
            &self.exclude_machines,
        );
        available
            && same_text(self.gpu.as_ref(), offer.gpu_name.as_ref())
            && same_count(self.num_gpus, offer.num_gpus)
            && at_least(offer.disk_space, disk)
            && at_least(offer.cuda_max_good, self.cuda)
            && at_least(offer.reliability, self.reliability)
            && at_least(offer.inet_up, self.inet_up)
            && at_least(offer.inet_down, self.inet_down)
            && at_least(offer.duration, min_secs)
            && price_ok
            && verified_ok
            && deverified_ok
            && country_ok
            && host_ok
            && machine_ok
    }

    /// The constraints in words, for the footer (so a silent config file shows); `no constraint`
    /// for the identity.
    ///
    /// ```
    /// use vast_skopos::offer::OfferQuery;
    /// assert_eq!(OfferQuery::default().summary(), "no constraint");
    /// let mut q = OfferQuery::default();
    /// q.gpu = Some("RTX 5090".to_owned());
    /// q.num_gpus = Some(1);
    /// q.reliability = Some(0.99);
    /// q.exclude_countries = vec!["CN".to_owned(), "cn".to_owned()];
    /// q.exclude_hosts = vec![362355, 362355];
    /// q.exclude_machines = vec![142875];
    /// assert_eq!(q.summary(),
    ///   "RTX 5090, 1 GPU, reliability >= 99 %, not in CN, not hosts 362355, not machines 142875");
    /// q.reliability = Some(0.57);
    /// assert!(q.summary().contains("reliability >= 57 %"), "no float noise");
    /// ```
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(g) = &self.gpu {
            // BORROW: explicit clone -- the summary owns its parts
            parts.push(g.clone());
        }
        if let Some(n) = self.num_gpus {
            parts.push(format!("{n} GPU{}", if n == 1 { "" } else { "s" }));
        }
        if let Some(d) = self.disk {
            parts.push(format!("disk >= {d} GB"));
        }
        if let Some(c) = self.cuda {
            parts.push(format!("CUDA >= {c}"));
        }
        if let Some(r) = self.reliability {
            parts.push(format!("reliability >= {} %", two_decimals(r * 100.0)));
        }
        if let Some(u) = self.inet_up {
            parts.push(format!("up >= {u} Mb/s"));
        }
        if let Some(d) = self.inet_down {
            parts.push(format!("down >= {d} Mb/s"));
        }
        if let Some(p) = self.max_price {
            parts.push(format!("<= ${p}/h"));
        }
        if let Some(d) = self.min_days {
            parts.push(format!(">= {d} days"));
        }
        if self.verified {
            parts.push(String::from("verified"));
        }
        if self.exclude_deverified {
            parts.push(String::from("not deverified"));
        }
        if !self.exclude_countries.is_empty() {
            parts.push(format!("not in {}", self.countries().join(" ")));
        }
        let named = |ids: &[u64]| {
            sorted_once(ids)
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        };
        if !self.only_hosts.is_empty() {
            parts.push(format!("hosts {}", named(&self.only_hosts)));
        }
        if !self.only_machines.is_empty() {
            parts.push(format!("machines {}", named(&self.only_machines)));
        }
        if !self.exclude_hosts.is_empty() {
            parts.push(format!("not hosts {}", named(&self.exclude_hosts)));
        }
        if !self.exclude_machines.is_empty() {
            parts.push(format!("not machines {}", named(&self.exclude_machines)));
        }
        if parts.is_empty() {
            String::from("no constraint")
        } else {
            parts.join(", ")
        }
    }
}

/// One answer of the search.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct OfferPage {
    /// The rows read.
    pub offers: Vec<Offer>,
    /// The rows the server answered, read or not.
    pub returned: usize,
    /// The rows skipped for want of an integer `id`.
    pub skipped: usize,
    /// The server's `truncated` flag, when it sent one (it has read `false` on a capped answer).
    pub truncated: Option<bool>,
}

impl OfferPage {
    /// Reads the search's answer; `None` without an `offers` list.
    ///
    /// ```
    /// use vast_skopos::offer::OfferPage;
    /// let page = OfferPage::from_json(&serde_json::json!(
    ///   {"offers": [{"id": 1}, {"gpu_name": "no id"}], "truncated": false})).unwrap();
    /// assert_eq!((page.offers.len(), page.returned, page.skipped, page.truncated), (1, 2, 1, Some(false)));
    /// assert!(OfferPage::from_json(&serde_json::json!({"rows": []})).is_none());
    /// ```
    #[must_use]
    pub fn from_json(answer: &Value) -> Option<Self> {
        let rows = answer.get("offers")?.as_array()?;
        let offers: Vec<Offer> = rows.iter().filter_map(Offer::from_json).collect();
        Some(Self {
            returned: rows.len(),
            skipped: rows.len() - offers.len(),
            offers,
            truncated: answer.get("truncated").and_then(Value::as_bool),
        })
    }

    /// Whether the answer may have been cut: the server says so, or it answered [`OFFER_CAP`] rows.
    ///
    /// ```
    /// use vast_skopos::offer::OfferPage;
    /// let rows: Vec<_> = (0..64).map(|id| serde_json::json!({"id": id})).collect();
    /// let capped = OfferPage::from_json(&serde_json::json!({"offers": rows, "truncated": false}));
    /// assert!(capped.unwrap().maybe_cut(), "64 rows: the cap, whatever `truncated` says");
    /// let said = OfferPage::from_json(&serde_json::json!({"offers": [], "truncated": true}));
    /// assert!(said.unwrap().maybe_cut());
    /// let whole = OfferPage::from_json(&serde_json::json!({"offers": [{"id": 1}]}));
    /// assert!(!whole.unwrap().maybe_cut());
    /// ```
    #[must_use]
    pub fn maybe_cut(&self) -> bool {
        self.truncated == Some(true) || self.returned >= OFFER_CAP
    }
}

/// The value `sort` orders by.
const fn sort_key(offer: &Offer, sort: OfferSort) -> Option<f64> {
    match sort {
        OfferSort::Price => offer.dph_total,
        OfferSort::Reliability => offer.reliability,
        OfferSort::Up => offer.inet_up,
    }
}

/// Two optional values, missing ones last; `ascending` or not for the present ones.
fn compare(a: Option<f64>, b: Option<f64>, ascending: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(x), Some(y)) if ascending => x.total_cmp(&y),
        (Some(x), Some(y)) => y.total_cmp(&x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// The offers `query` admits, ordered by `sort` (price ascending, reliability and upload
/// descending), missing values last, ties by price and then id; an id listed twice keeps the
/// row that sorts first.
///
/// ```
/// use vast_skopos::offer::{Offer, OfferQuery, OfferSort, select};
/// let rows = [(3, 0.60), (1, 0.55), (2, 0.55)];
/// let offers: Vec<Offer> = rows.iter()
///     .map(|&(id, p)| Offer::from_json(&serde_json::json!({"id": id, "dph_total": p})).unwrap())
///     .chain([Offer::from_json(&serde_json::json!({"id": 9})).unwrap()])
///     .collect();
/// let ids: Vec<u64> = select(offers, &OfferQuery::default(), OfferSort::Price).iter().map(|o| o.id).collect();
/// assert_eq!(ids, vec![1, 2, 3, 9]);
/// // An id answered twice is one offer: the cheaper row stays.
/// let twice = [(7, 0.9), (7, 0.5)].map(|(id, p)| Offer::from_json(&serde_json::json!({"id": id, "dph_total": p})).unwrap());
/// let kept = select(twice.to_vec(), &OfferQuery::default(), OfferSort::Price);
/// assert_eq!(kept.iter().map(|o| o.dph_total).collect::<Vec<_>>(), [Some(0.5)]);
/// ```
#[must_use]
pub fn select(offers: Vec<Offer>, query: &OfferQuery, sort: OfferSort) -> Vec<Offer> {
    let mut kept: Vec<Offer> = offers.into_iter().filter(|o| query.admits(o)).collect();
    let ascending = matches!(sort, OfferSort::Price);
    kept.sort_by(|a, b| {
        compare(sort_key(a, sort), sort_key(b, sort), ascending)
            .then_with(|| compare(a.dph_total, b.dph_total, true))
            .then(a.id.cmp(&b.id))
    });
    let mut seen = std::collections::HashSet::new();
    kept.retain(|o| seen.insert(o.id));
    kept
}

/// An offer whose price moved since a watch last saw it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Repriced {
    /// The offer as it reads now.
    pub offer: Offer,
    /// Its price when last seen, in dollars per hour.
    pub was: f64,
}

/// What moved in the market since a watch last saw it ([`crate::market::MarketWatch::record`]).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct OfferChanges {
    /// The machines' offers that are new: absent from the previous looks, in the look's order.
    pub appeared: Vec<Offer>,
    /// The offers whose price (to the thousandth of a dollar, as shown) moved since last seen.
    pub repriced: Vec<Repriced>,
    /// The offers missing for several looks in a row (rented, withdrawn, or no longer matching),
    /// cheapest first.
    pub gone: Vec<Offer>,
}

impl OfferChanges {
    /// Whether nothing moved.
    ///
    /// ```
    /// use vast_skopos::offer::OfferChanges;
    /// assert!(OfferChanges::default().is_empty());
    /// ```
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.appeared.is_empty() && self.repriced.is_empty() && self.gone.is_empty()
    }
}

/// A GPU model as vast.ai names it: trimmed, `_` read as a space (the official CLI's rule, so
/// `RTX_5090` needs no quotes); `None` when blank or quoted.
///
/// ```
/// use vast_skopos::offer::gpu_model;
/// assert_eq!(gpu_model(" RTX_5090 ").as_deref(), Some("RTX 5090"));
/// assert_eq!(gpu_model("H100 SXM").as_deref(), Some("H100 SXM"));
/// assert_eq!((gpu_model(""), gpu_model("\"RTX 5090\"")), (None, None));
/// ```
#[must_use]
pub fn gpu_model(name: &str) -> Option<String> {
    let model = name.trim().replace('_', " ");
    (!model.is_empty() && !model.contains(['"', '\''])).then_some(model)
}

/// A country code: two ASCII letters, upper-cased; `None` otherwise.
///
/// ```
/// use vast_skopos::offer::country_code;
/// assert_eq!(country_code(" cn ").as_deref(), Some("CN"));
/// assert_eq!((country_code("China"), country_code("C1")), (None, None));
/// ```
#[must_use]
pub fn country_code(code: &str) -> Option<String> {
    let code = code.trim();
    is_country_code(code).then(|| code.to_ascii_uppercase())
}

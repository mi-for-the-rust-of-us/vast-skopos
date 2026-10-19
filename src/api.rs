//! The three vast.ai endpoints `vas` reads, and nothing else: it never writes, starts, stops or
//! destroys anything.
//!
//! - `GET /api/v1/instances/`, paginated (`limit`, `order_by`, then `after_token` from each page's
//!   `next_token`), as the official CLI's `show instances` does since the v0 list was deprecated;
//! - `GET /api/v0/users/current/`, for the credit;
//! - `GET /api/v0/bundles/?q=<JSON>`, the offer search, sent WITHOUT any key by a [`SearchClient`]
//!   (see [`crate::offer`]).
//!
//! The base URL is `https://console.vast.ai`, or `VAST_URL` when set (the CLI's own override).
//!
//! vast.ai rate-limits each endpoint and says so in the `X-RateLimit-*` headers of its answers (seen
//! 2026-10-18: 30 calls for the offer search, ONE for the instances). The clients keep what each
//! answer said ([`RateLimit`]), and [`Client::budget_wait_secs`] tells a caller that loops how long
//! to leave vast.ai alone; a refusal is [`SkoposError::RateLimited`], with the wait it named.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::error::{Result, SkoposError};
use crate::instance::{Account, Instance};
use crate::key::ApiKey;
use crate::offer::{OfferPage, OfferQuery, OfferSort};

/// The default API host.
pub const DEFAULT_BASE: &str = "https://console.vast.ai";
/// Rows per instances page (the official CLI's).
const PAGE: &str = "25";
/// Pages fetched at most: a guard against a `next_token` that never ends, not a real limit.
const MAX_PAGES: usize = 200;

/// The HTTP agent: one connection pool, a 30 s timeout per request, non-success statuses returned
/// rather than raised, and no redirect followed (a 3xx is reported, and a key is never re-sent
/// elsewhere).
///
/// The proxy of the environment (`HTTPS_PROXY`, ...) is honoured, except for a loopback `base`
/// (the tests' mock, a local tunnel): what runs on this machine is not sent to a proxy.
fn agent(base: &str) -> ureq::Agent {
    let mut config = ureq::Agent::config_builder();
    config = config
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .max_redirects(0);
    if is_loopback(base) {
        config = config.proxy(None);
    }
    config.build().into()
}

/// Whether `base` (a URL) names this machine: `127.0.0.1`, `localhost` or `[::1]`, in any case.
///
/// The authority ends at the first `/`, `?` or `#`: in `http://evil.example?@127.0.0.1` the host is
/// `evil.example`, whatever follows the `?`.
fn is_loopback(base: &str) -> bool {
    let host = base.split_once("://").map_or(base, |(_, rest)| rest);
    let host = host.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = if host.starts_with('[') {
        host.split_once(']')
            .map_or(host, |(h, _)| h.trim_start_matches('['))
    } else {
        host.split(':').next().unwrap_or_default()
    };
    // BORROW: explicit as_str -- the lower-cased host is matched against string literals
    matches!(
        host.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1"
    )
}

/// `VAST_URL` when set and not blank, else [`DEFAULT_BASE`]; without a trailing slash.
fn base_from_env() -> String {
    let base = std::env::var("VAST_URL")
        .ok()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| String::from(DEFAULT_BASE));
    // BORROW: explicit to_owned -- the client owns its trimmed base URL
    base.trim_end_matches('/').to_owned()
}

/// The hint shared by every endpoint: a redirect, which `vas` never follows; empty otherwise.
const fn redirect_hint(status: u16) -> &'static str {
    match status {
        301 | 302 | 303 | 307 | 308 => " (a redirect: vas follows none)",
        _ => "",
    }
}

/// The hint for a keyed endpoint's status.
const fn keyed_hint(status: u16) -> &'static str {
    match status {
        401 | 403 => " (is the API key right, and not revoked?)",
        _ => redirect_hint(status),
    }
}

/// The hint for the offer search's status.
const fn search_hint(status: u16) -> &'static str {
    match status {
        401 | 403 => {
            " (the offer search asked for a key: vas offers sends none, and needs an update)"
        }
        404 | 405 => {
            " (the legacy GET form of the offer search may be gone: vas offers needs an update)"
        }
        _ => redirect_hint(status),
    }
}

/// What vast.ai says about the request budget of an endpoint, in the `X-RateLimit-*` headers of
/// its answers.
///
/// Seen 2026-10-18: the offer search allowed 30 calls, the instances list ONE, per window.
///
/// ```
/// use vast_skopos::api::RateLimit;
/// let headers = |name: &str| match name {
///     "x-ratelimit-limit" => Some("1".to_owned()),
///     "x-ratelimit-remaining" => Some("0".to_owned()),
///     "x-ratelimit-reset" => Some("1792318828".to_owned()),
///     _ => None,
/// };
/// let limit = RateLimit::from_headers(headers);
/// assert_eq!(limit.wait_secs(1792318820.0), Some(8.0), "spent: wait for the reset");
/// assert_eq!(limit.wait_secs(1792318830.0), None, "the reset has passed");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct RateLimit {
    /// The calls the window allows (`X-RateLimit-Limit`; the API writes `30.0` or `1`).
    pub limit: Option<f64>,
    /// The calls left in it (`X-RateLimit-Remaining`).
    pub remaining: Option<f64>,
    /// When the budget is renewed, in Unix seconds (`X-RateLimit-Reset`).
    pub reset_at: Option<f64>,
}

impl RateLimit {
    /// Reads the three headers through `header` (given a LOWER-CASE name, returns its value); a
    /// header that is absent or not a finite number is `None`.
    #[must_use]
    pub fn from_headers(header: impl Fn(&str) -> Option<String>) -> Self {
        let number = |name: &str| {
            header(name)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|n| n.is_finite())
        };
        Self {
            limit: number("x-ratelimit-limit"),
            remaining: number("x-ratelimit-remaining"),
            reset_at: number("x-ratelimit-reset"),
        }
    }

    /// Whether the answer said anything about a budget.
    #[must_use]
    pub const fn is_known(&self) -> bool {
        self.limit.is_some() || self.remaining.is_some() || self.reset_at.is_some()
    }

    /// The seconds from `now` (Unix) until a call is allowed again: `Some` only when the budget is
    /// spent (nothing remaining) and its reset is still ahead.
    #[must_use]
    pub fn wait_secs(&self, now: f64) -> Option<f64> {
        let spent = self.remaining.is_some_and(|r| r < 1.0);
        let reset = self.reset_at?;
        (spent && reset > now).then_some(reset - now)
    }
}

/// What each endpoint last said about its budget, by path.
#[derive(Debug, Default)]
struct Limits(Mutex<HashMap<String, RateLimit>>);

impl Limits {
    /// Remembers what `path` said, when it said anything.
    fn record(&self, path: &str, limit: RateLimit) {
        if limit.is_known()
            && let Ok(mut by_path) = self.0.lock()
        {
            // BORROW: explicit to_owned -- the map owns its keys
            by_path.insert(path.to_owned(), limit);
        }
    }

    /// The longest wait any endpoint asks for at `now` (Unix seconds).
    fn wait_secs(&self, now: f64) -> Option<f64> {
        let by_path = self.0.lock().ok()?;
        by_path
            .values()
            .filter_map(|limit| limit.wait_secs(now))
            .max_by(f64::total_cmp)
    }
}

/// Unix seconds now.
fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// `GET {base}{path}` with `query`, as JSON; the key as a bearer only when `bearer` is given;
/// `hint` words a non-success status. What the answer says about the budget goes to `limits`.
fn get_json(
    agent: &ureq::Agent,
    limits: &Limits,
    base: &str,
    path: &str,
    query: &[(&str, &str)],
    bearer: Option<&ApiKey>,
    hint: fn(u16) -> &'static str,
) -> Result<Value> {
    let url = format!("{base}{path}");
    let mut request = agent.get(&url);
    if let Some(key) = bearer {
        request = request.header("Authorization", &key.bearer());
    }
    for &(k, v) in query {
        request = request.query(k, v);
    }
    let mut response = request
        .call()
        .map_err(|e| SkoposError::http(path, e.to_string()))?;
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let budget = RateLimit::from_headers(header);
    limits.record(path, budget);
    let status = response.status().as_u16();
    if status == 429 {
        let named = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|n| n.is_finite() && *n >= 0.0);
        return Err(SkoposError::rate_limited(
            path,
            named.or_else(|| budget.wait_secs(unix_now())),
        ));
    }
    if !(200..300).contains(&status) {
        return Err(SkoposError::status(path, status, hint(status)));
    }
    response
        .body_mut()
        .read_json::<Value>()
        .map_err(|e| SkoposError::json(path, e.to_string()))
}

/// A read-only vast.ai API client, with the account's key.
#[derive(Debug)]
pub struct Client {
    /// The HTTP agent.
    agent: ureq::Agent,
    /// The key, redacted in `Debug`.
    key: ApiKey,
    /// The base URL, without a trailing slash.
    base: String,
    /// What each endpoint last said about its request budget.
    limits: Limits,
}

impl Client {
    /// A client for `key`, against `VAST_URL` or [`DEFAULT_BASE`], with a 30 s timeout per request.
    #[must_use]
    pub fn new(key: ApiKey) -> Self {
        Self::with_base(key, &base_from_env())
    }

    /// A client for `key` against `base` (e.g. a test server), with the same timeout.
    #[must_use]
    pub fn with_base(key: ApiKey, base: &str) -> Self {
        // BORROW: explicit to_owned -- the client owns its base URL
        let base = base.trim_end_matches('/').to_owned();
        Self {
            agent: agent(&base),
            key,
            base,
            limits: Limits::default(),
        }
    }

    /// The base URL the key is sent to.
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    /// `GET {base}{path}` with `query` and the key, as JSON.
    fn get_json(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        get_json(
            &self.agent,
            &self.limits,
            &self.base,
            path,
            query,
            Some(&self.key),
            keyed_hint,
        )
    }

    /// How many seconds to leave vast.ai alone, from what its last answers said: the longest wait
    /// of any endpoint whose request budget is spent; `None` when no budget is spent. A caller that
    /// loops sleeps at least this long between two looks.
    #[must_use]
    pub fn budget_wait_secs(&self) -> Option<f64> {
        self.limits.wait_secs(unix_now())
    }

    /// Every instance of the account, all pages.
    ///
    /// # Errors
    /// Returns [`SkoposError::Http`] if a request cannot be made.
    /// Returns [`SkoposError::RateLimited`] if vast.ai answers 429.
    /// Returns [`SkoposError::Status`] if vast.ai answers with another non-success status.
    /// Returns [`SkoposError::Json`] if a page is not JSON, has no `instances` list, repeats a
    /// `next_token`, or the pages exceed the guard.
    pub fn instances(&self) -> Result<Vec<Instance>> {
        self.instances_with(|_| false)
    }

    /// Every instance of the account, all pages, with a say in what happens when the budget is spent
    /// BETWEEN two pages.
    ///
    /// vast.ai allows the instances list one call per window (about 3 s, seen 2026-10-18), and a
    /// page holds 25 rows: an account with more instances needs a second call. When the budget of
    /// the list is spent before a later page, `between_pages` is asked, with the seconds to wait:
    /// `true` means the caller has waited that long (this library never sleeps) and the page is
    /// requested; `false` ends the call with [`SkoposError::RateLimited`], naming the wait, instead
    /// of spending a request that would be refused. [`Client::instances`] always answers `false`.
    ///
    /// # Errors
    /// The same as [`Client::instances`], and [`SkoposError::RateLimited`] when `between_pages`
    /// answers `false`.
    pub fn instances_with(
        &self,
        mut between_pages: impl FnMut(f64) -> bool,
    ) -> Result<Vec<Instance>> {
        let path = "/api/v1/instances/";
        let mut rows = Vec::new();
        let mut after: Option<String> = None;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..MAX_PAGES {
            let mut query = vec![
                ("select_filters", "{}"),
                ("order_by", r#"[{"col": "id", "dir": "asc"}]"#),
                ("limit", PAGE),
            ];
            if let Some(token) = after.as_deref() {
                query.push(("after_token", token));
                if let Some(wait) = self.limits.wait_secs(unix_now())
                    && !between_pages(wait)
                {
                    return Err(SkoposError::rate_limited(path, Some(wait)));
                }
            }
            let page = self.get_json(path, &query)?;
            let list = page
                .get("instances")
                .and_then(Value::as_array)
                .ok_or_else(|| SkoposError::json(path, "no `instances` list"))?;
            if list.is_empty() {
                return Ok(rows);
            }
            rows.extend(list.iter().filter_map(Instance::from_json));
            let token = match page.get("next_token") {
                // BORROW: explicit clone -- the token outlives the page it came in
                Some(Value::String(t)) if !t.is_empty() => t.clone(),
                Some(Value::Number(n)) => n.to_string(),
                Some(_) | None => return Ok(rows),
            };
            if !seen.insert(token.clone()) {
                return Err(SkoposError::json(
                    path,
                    format!("next_token {token} repeated"),
                ));
            }
            after = Some(token);
        }
        Err(SkoposError::json(
            path,
            format!("more than {MAX_PAGES} pages"),
        ))
    }

    /// The account's credit and balance.
    ///
    /// # Errors
    /// Returns [`SkoposError::Http`] if the request cannot be made.
    /// Returns [`SkoposError::RateLimited`] if vast.ai answers 429.
    /// Returns [`SkoposError::Status`] if vast.ai answers with another non-success status.
    /// Returns [`SkoposError::Json`] if the answer is not JSON.
    pub fn account(&self) -> Result<Account> {
        // The trailing slash is the canonical path: without it the API answers 301 (dogfooding,
        // 2026-10-16), and `vas` follows no redirect.
        self.get_json("/api/v0/users/current/", &[])
            .map(|user| Account::from_json(&user))
    }
}

/// A client for the offer search alone. It holds no key and sends none: the search is public.
#[derive(Debug)]
pub struct SearchClient {
    /// The HTTP agent.
    agent: ureq::Agent,
    /// The base URL, without a trailing slash.
    base: String,
    /// What the search last said about its request budget.
    limits: Limits,
}

impl Default for SearchClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchClient {
    /// A search client against `VAST_URL` or [`DEFAULT_BASE`], with a 30 s timeout per request.
    #[must_use]
    pub fn new() -> Self {
        Self::with_base(&base_from_env())
    }

    /// A search client against `base` (e.g. a test server), with the same timeout.
    ///
    /// ```
    /// use vast_skopos::api::SearchClient;
    /// assert_eq!(SearchClient::with_base("http://127.0.0.1:9/").base(), "http://127.0.0.1:9");
    /// assert!(SearchClient::new().base().starts_with("http"));
    /// ```
    #[must_use]
    pub fn with_base(base: &str) -> Self {
        // BORROW: explicit to_owned -- the client owns its base URL
        let base = base.trim_end_matches('/').to_owned();
        Self {
            agent: agent(&base),
            base,
            limits: Limits::default(),
        }
    }

    /// The base URL searched.
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    /// How many seconds to leave the search alone, from what its last answer said; `None` when its
    /// request budget is not spent (see [`Client::budget_wait_secs`]).
    #[must_use]
    pub fn budget_wait_secs(&self) -> Option<f64> {
        self.limits.wait_secs(unix_now())
    }

    /// One search: the offers `query` asks for, in the server's `sort` order (at most
    /// [`crate::offer::OFFER_CAP`] rows, whatever is asked). The rows are NOT re-checked here:
    /// [`crate::offer::select`] does that.
    ///
    /// # Errors
    /// Returns [`SkoposError::Http`] if the request cannot be made.
    /// Returns [`SkoposError::RateLimited`] if vast.ai answers 429.
    /// Returns [`SkoposError::Status`] if vast.ai answers with another non-success status.
    /// Returns [`SkoposError::Json`] if the answer is not JSON or has no `offers` list.
    pub fn offers(&self, query: &OfferQuery, sort: OfferSort) -> Result<OfferPage> {
        let path = "/api/v0/bundles/";
        let q = query.to_search_json(sort).to_string();
        let answer = get_json(
            &self.agent,
            &self.limits,
            &self.base,
            path,
            &[("q", &q)],
            None,
            search_hint,
        )?;
        OfferPage::from_json(&answer).ok_or_else(|| SkoposError::json(path, "no `offers` list"))
    }
}

#[cfg(test)]
mod tests {
    use super::is_loopback;

    #[test]
    fn only_this_machine_is_loopback() {
        for local in [
            "http://127.0.0.1:8080",
            "http://127.0.0.1",
            "http://localhost/api",
            "http://localhost:9",
            "https://user:pw@127.0.0.1:1/x",
            "http://[::1]:9",
            "127.0.0.1:9",
            "http://LOCALHOST:9",
            "http://127.0.0.1?x=1",
            "http://localhost#frag",
        ] {
            assert!(is_loopback(local), "{local}");
        }
        for remote in [
            "https://console.vast.ai",
            "http://127.0.0.1.evil.example",
            "http://localhost.evil.example/",
            "http://evil.example/127.0.0.1",
            "http://evil.example@console.vast.ai",
            "http://evil.example?@127.0.0.1",
            "http://evil.example#@localhost",
            "",
        ] {
            assert!(!is_loopback(remote), "{remote}");
        }
    }
}

//! The three vast.ai endpoints `vas` reads, and nothing else: it never writes, starts, stops or
//! destroys anything.
//!
//! - `GET /api/v1/instances/`, paginated (`limit`, `order_by`, then `after_token` from each page's
//!   `next_token`), as the official CLI's `show instances` does since the v0 list was deprecated;
//! - `GET /api/v0/users/current`, for the credit;
//! - `GET /api/v0/bundles/?q=<JSON>`, the offer search, sent WITHOUT any key by a [`SearchClient`]
//!   (see [`crate::offer`]).
//!
//! The base URL is `https://console.vast.ai`, or `VAST_URL` when set (the CLI's own override).

use std::time::Duration;

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

/// Whether `base` (a URL) names this machine: `127.0.0.1`, `localhost` or `[::1]`.
fn is_loopback(base: &str) -> bool {
    let host = base.split_once("://").map_or(base, |(_, rest)| rest);
    let host = host.split('/').next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = if host.starts_with('[') {
        host.split_once(']')
            .map_or(host, |(h, _)| h.trim_start_matches('['))
    } else {
        host.split(':').next().unwrap_or_default()
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
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
        429 => " (rate-limited: wait before the next call)",
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
        429 => " (rate-limited: wait before the next call)",
        _ => redirect_hint(status),
    }
}

/// `GET {base}{path}` with `query`, as JSON; the key as a bearer only when `bearer` is given;
/// `hint` words a non-success status.
fn get_json(
    agent: &ureq::Agent,
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
    let status = response.status().as_u16();
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
            &self.base,
            path,
            query,
            Some(&self.key),
            keyed_hint,
        )
    }

    /// Every instance of the account, all pages.
    ///
    /// # Errors
    /// Returns [`SkoposError::Http`] if a request cannot be made.
    /// Returns [`SkoposError::Status`] if vast.ai answers with a non-success status.
    /// Returns [`SkoposError::Json`] if a page is not JSON, has no `instances` list, repeats a
    /// `next_token`, or the pages exceed the guard.
    pub fn instances(&self) -> Result<Vec<Instance>> {
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
    /// Returns [`SkoposError::Status`] if vast.ai answers with a non-success status.
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
        }
    }

    /// The base URL searched.
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    /// One search: the offers `query` asks for, in the server's `sort` order (at most
    /// [`crate::offer::OFFER_CAP`] rows, whatever is asked). The rows are NOT re-checked here:
    /// [`crate::offer::select`] does that.
    ///
    /// # Errors
    /// Returns [`SkoposError::Http`] if the request cannot be made.
    /// Returns [`SkoposError::Status`] if vast.ai answers with a non-success status.
    /// Returns [`SkoposError::Json`] if the answer is not JSON or has no `offers` list.
    pub fn offers(&self, query: &OfferQuery, sort: OfferSort) -> Result<OfferPage> {
        let path = "/api/v0/bundles/";
        let q = query.to_search_json(sort).to_string();
        let answer = get_json(
            &self.agent,
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
        ] {
            assert!(is_loopback(local), "{local}");
        }
        for remote in [
            "https://console.vast.ai",
            "http://127.0.0.1.evil.example",
            "http://localhost.evil.example/",
            "http://evil.example/127.0.0.1",
            "http://evil.example@console.vast.ai",
            "",
        ] {
            assert!(!is_loopback(remote), "{remote}");
        }
    }
}

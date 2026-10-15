//! The two vast.ai endpoints `vas` reads, and nothing else: it never writes, starts, stops or
//! destroys anything.
//!
//! - `GET /api/v1/instances/`, paginated (`limit`, `order_by`, then `after_token` from each page's
//!   `next_token`), as the official CLI's `show instances` does since the v0 list was deprecated;
//! - `GET /api/v0/users/current`, for the credit.
//!
//! The base URL is `https://console.vast.ai`, or `VAST_URL` when set (the CLI's own override).

use std::time::Duration;

use serde_json::Value;

use crate::error::{Result, SkoposError};
use crate::instance::{Account, Instance};
use crate::key::ApiKey;

/// The default API host.
pub const DEFAULT_BASE: &str = "https://console.vast.ai";
/// Rows per instances page (the official CLI's).
const PAGE: &str = "25";
/// Pages fetched at most: a guard against a `next_token` that never ends, not a real limit.
const MAX_PAGES: usize = 200;

/// The endpoint path, owned, for an error (never the full URL: no query, no key).
fn endpoint(path: &str) -> String {
    // BORROW: explicit to_owned -- the error outlives the request that named the path
    path.to_owned()
}

/// A read-only vast.ai API client.
#[derive(Debug)]
pub struct Client {
    /// The HTTP agent (one connection pool, a global timeout).
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
        let base = std::env::var("VAST_URL")
            .ok()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| String::from(DEFAULT_BASE));
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            // No redirect is followed: a 3xx is reported, and the key is never re-sent elsewhere.
            .max_redirects(0)
            .build()
            .into();
        Self {
            agent,
            key,
            // BORROW: explicit to_owned -- the client owns its trimmed base URL
            base: base.trim_end_matches('/').to_owned(),
        }
    }

    /// A client for `key` against `base` (e.g. a test server), with the same timeout.
    #[must_use]
    pub fn with_base(key: ApiKey, base: &str) -> Self {
        let mut client = Self::new(key);
        base.trim_end_matches('/').clone_into(&mut client.base);
        client
    }

    /// The base URL the key is sent to.
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    /// `GET {base}{path}` with `query`, as JSON.
    fn get_json(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let url = format!("{}{path}", self.base);
        let mut request = self
            .agent
            .get(&url)
            .header("Authorization", &self.key.bearer());
        for &(k, v) in query {
            request = request.query(k, v);
        }
        let mut response = request.call().map_err(|e| SkoposError::Http {
            endpoint: endpoint(path),
            message: format!("{e}"),
        })?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let hint = match status {
                401 | 403 => " (is the API key right, and not revoked?)",
                429 => " (rate-limited: wait before the next call)",
                _ => "",
            };
            return Err(SkoposError::Status {
                endpoint: endpoint(path),
                status,
                hint,
            });
        }
        response
            .body_mut()
            .read_json::<Value>()
            .map_err(|e| SkoposError::Json {
                endpoint: endpoint(path),
                message: format!("{e}"),
            })
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
                .ok_or_else(|| SkoposError::Json {
                    endpoint: endpoint(path),
                    message: String::from("no `instances` list"),
                })?;
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
                return Err(SkoposError::Json {
                    endpoint: endpoint(path),
                    message: format!("next_token {token} repeated"),
                });
            }
            after = Some(token);
        }
        Err(SkoposError::Json {
            endpoint: endpoint(path),
            message: format!("more than {MAX_PAGES} pages"),
        })
    }

    /// The account's credit and balance.
    ///
    /// # Errors
    /// Returns [`SkoposError::Http`] if the request cannot be made.
    /// Returns [`SkoposError::Status`] if vast.ai answers with a non-success status.
    /// Returns [`SkoposError::Json`] if the answer is not JSON.
    pub fn account(&self) -> Result<Account> {
        self.get_json("/api/v0/users/current", &[])
            .map(|user| Account::from_json(&user))
    }
}

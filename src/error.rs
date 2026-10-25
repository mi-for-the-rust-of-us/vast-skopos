//! The crate's error type.

/// Everything `vast-skopos` can fail on.
///
/// No variant ever carries the API key: errors are printed, and a printed key is a leaked key.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SkoposError {
    /// No API key in `VAST_API_KEY` nor in any of the files searched.
    #[error("no vast.ai API key found (set VAST_API_KEY or write it to one of: {searched}){hint}")]
    NoApiKey {
        /// The files searched, in order, joined for display.
        searched: String,
        /// Empty, or a parenthetical starting with a space that names a searched file's `.txt`
        /// twin (Windows Notepad appends `.txt`).
        hint: String,
    },

    /// The API key file exists but cannot be read.
    #[error("failed to read the API key file {path}: {source}")]
    KeyFile {
        /// The file.
        path: String,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The `vas offers` config file cannot be read (or, named by `--config`, does not exist).
    #[error("failed to read the config file {path}: {source}")]
    ConfigFile {
        /// The file.
        path: String,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// A line of the `vas offers` config file is refused.
    #[error("line {line} of the config file {path} is refused: {text:?} ({problem})")]
    ConfigLine {
        /// The file.
        path: String,
        /// The line's number, from 1.
        line: usize,
        /// The line, trimmed (shown escaped).
        text: String,
        /// What is wrong with it.
        problem: String,
    },

    /// vast.ai answered with a non-success HTTP status.
    #[error("vast.ai returned status {status} for {endpoint}{hint}")]
    Status {
        /// The endpoint path (no query, no key).
        endpoint: String,
        /// The HTTP status code.
        status: u16,
        /// A short hint for the common statuses (empty otherwise).
        hint: &'static str,
    },

    /// vast.ai refused the request for its rate limit (HTTP 429): every endpoint has a budget of
    /// calls per window, and says so in `X-RateLimit-*` headers.
    #[error("vast.ai rate-limited {endpoint}: {}", wait_text(.retry_after.as_ref()))]
    RateLimited {
        /// The endpoint path (no query, no key).
        endpoint: String,
        /// How many seconds to wait before the next call, when vast.ai said (`Retry-After`, else
        /// the time to the budget's reset).
        retry_after: Option<f64>,
    },

    /// The request could not be made or its body could not be read.
    #[error("failed to request {endpoint}: {message}")]
    Http {
        /// The endpoint path (no query, no key).
        endpoint: String,
        /// What went wrong.
        message: String,
    },

    /// The response was not the JSON expected.
    #[error("response from {endpoint} is not the expected JSON ({message})")]
    Json {
        /// The endpoint path (no query, no key).
        endpoint: String,
        /// What was missing or malformed.
        message: String,
    },

    /// The `ssh` probe of one box failed.
    #[error("failed to probe {target} over ssh: {message}")]
    Ssh {
        /// `host:port`.
        target: String,
        /// The reason (`ssh`'s own last line when it gave one).
        message: String,
    },
}

/// The crate's result type.
pub type Result<T> = std::result::Result<T, SkoposError>;

impl SkoposError {
    /// A request to `path` that could not be made. Only the path, never the URL with its query:
    /// an error is printed, and a printed key is a leaked key.
    pub(crate) fn http(path: &str, message: impl Into<String>) -> Self {
        Self::Http {
            // BORROW: explicit to_owned -- the error outlives the request that named the path
            endpoint: path.to_owned(),
            message: message.into(),
        }
    }

    /// An answer from `path` that is not the JSON expected.
    pub(crate) fn json(path: &str, message: impl Into<String>) -> Self {
        Self::Json {
            // BORROW: explicit to_owned -- the error outlives the request that named the path
            endpoint: path.to_owned(),
            message: message.into(),
        }
    }

    /// The `ssh` probe of `target` (`host:port`) failed, for `message`.
    pub(crate) fn ssh(target: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Ssh {
            target: target.into(),
            message: message.into(),
        }
    }

    /// A rate-limit refusal (HTTP 429) from `path`, with the wait vast.ai named, if it did.
    pub(crate) fn rate_limited(path: &str, retry_after: Option<f64>) -> Self {
        Self::RateLimited {
            // BORROW: explicit to_owned -- the error outlives the request that named the path
            endpoint: path.to_owned(),
            retry_after,
        }
    }

    /// A non-success `status` from `path`, with a `hint` when the status is a common one.
    pub(crate) fn status(path: &str, status: u16, hint: &'static str) -> Self {
        Self::Status {
            // BORROW: explicit to_owned -- the error outlives the request that named the path
            endpoint: path.to_owned(),
            status,
            hint,
        }
    }

    /// Whether this is an `ssh` probe that never reached the box: a [`SkoposError::Ssh`] whose reason
    /// is a failure of the connection itself (refused, timed out, no route, a name that does not
    /// resolve, closed or reset before the session, or no answer within the probe's deadline).
    ///
    /// A box that answered and then refused the key, changed its host key or gave no GPU reading
    /// WAS reached: `false`, as for every other variant. The difference matters to the decision the
    /// table serves: an unreachable box of a few minutes may still come up, a refused key will not.
    ///
    /// ```
    /// use vast_skopos::SkoposError;
    /// let ssh = |m: &str| SkoposError::Ssh { target: "ssh3.vast.ai:32456".to_owned(), message: m.to_owned() };
    /// assert!(ssh("ssh: connect to host ssh3.vast.ai port 32456: Connection refused").is_unreachable());
    /// assert!(ssh("kex_exchange_identification: Connection closed by remote host").is_unreachable());
    /// assert!(!ssh("root@ssh3.vast.ai: Permission denied (publickey).").is_unreachable());
    /// assert!(!SkoposError::Http { endpoint: "/".to_owned(), message: "timed out".to_owned() }.is_unreachable());
    /// ```
    #[must_use]
    pub fn is_unreachable(&self) -> bool {
        if let Self::Ssh { message, .. } = self {
            let reason = message.to_ascii_lowercase();
            UNREACHABLE_WORDS.iter().any(|words| reason.contains(words))
        } else {
            false
        }
    }
}

/// What a probe's reason says, lower-cased, when the box was never reached: OpenSSH's own words,
/// and the probe's deadline.
const UNREACHABLE_WORDS: [&str; 10] = [
    // Nothing listens on the port: a vast.ai proxy with no box behind it yet.
    "connection refused",
    // `Connection timed out`, `Operation timed out`, `... timed out during banner exchange`.
    "timed out",
    "no route to host",
    "network is unreachable",
    // `Could not resolve hostname X: Name or service not known` (or another resolver's reason).
    "could not resolve hostname",
    "name or service not known",
    // The proxy hung up before the session: `Connection closed by 1.2.3.4 port 22`.
    "connection closed",
    "connection reset",
    // `kex_exchange_identification: read: Connection reset by peer`, and its other endings.
    "kex_exchange_identification",
    // The probe's own deadline (`probe::ssh_probe`): ssh never answered.
    "no answer within",
];

/// The advice of a rate-limit error: how long to wait, rounded up to the second, or just to wait.
fn wait_text(retry_after: Option<&f64>) -> String {
    retry_after.map_or_else(
        || String::from("wait before the next call"),
        |secs| format!("try again in {:.0} s", secs.ceil().max(1.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::SkoposError;

    fn ssh(message: &str) -> SkoposError {
        SkoposError::ssh("ssh3.vast.ai:32456", message)
    }

    #[test]
    fn what_openssh_says_of_a_box_it_never_reached_reads_as_unreachable() {
        for message in [
            "ssh: connect to host ssh3.vast.ai port 32456: Connection refused",
            "ssh: connect to host 185.65.225.164 port 42722: Connection timed out",
            "ssh: connect to host 185.65.225.164 port 42722: Operation timed out",
            "Connection timed out during banner exchange",
            "ssh: connect to host 10.0.0.1 port 22: No route to host",
            "ssh: connect to host 10.0.0.1 port 22: Network is unreachable",
            "ssh: Could not resolve hostname ssh9.vast.ai: Name or service not known",
            "ssh: Could not resolve hostname ssh9.vast.ai: No such host is known.",
            "Connection closed by 185.65.225.164 port 42722",
            "kex_exchange_identification: read: Connection reset by peer",
            "kex_exchange_identification: Connection closed by remote host",
            "no answer within 30 s (ssh killed)",
        ] {
            assert!(ssh(message).is_unreachable(), "{message}");
        }
    }

    #[test]
    fn a_box_that_answered_and_refused_was_reached() {
        for message in [
            "root@ssh3.vast.ai: Permission denied (publickey).",
            "Host key verification failed.",
            "no GPU reading (ssh exit 0)",
            "the probe thread panicked",
            "failed to run ssh: program not found",
        ] {
            assert!(!ssh(message).is_unreachable(), "{message}");
        }
        let http = SkoposError::http("/api/v1/instances/", "connection refused");
        assert!(!http.is_unreachable(), "only a probe can be unreachable");
    }
}

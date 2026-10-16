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
    Config {
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

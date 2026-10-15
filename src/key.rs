//! The vast.ai API key: where it is found, and how it is kept out of every output.
//!
//! The search order is the official `vastai` CLI's, so a key it saved is found here:
//!
//! 1. the `VAST_API_KEY` environment variable;
//! 2. `$XDG_CONFIG_HOME/vastai/vast_api_key`, when `XDG_CONFIG_HOME` is set;
//! 3. `~/.config/vastai/vast_api_key`;
//! 4. `~/.vast_api_key` (the CLI's legacy location).

use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::{Result, SkoposError};

/// A vast.ai API key. Its `Debug` output is redacted, and it has no `Display`.
///
/// ```
/// use vast_skopos::key::ApiKey;
/// let key = ApiKey::new("0123456789abcdef").unwrap();
/// assert_eq!(format!("{key:?}"), "ApiKey(<redacted>)");
/// assert!(ApiKey::new("   ").is_none());
/// ```
#[derive(Clone)]
pub struct ApiKey(String);

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

impl ApiKey {
    /// A key from its text, trimmed; `None` when nothing is left.
    #[must_use]
    pub fn new(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        // BORROW: explicit to_owned -- the key outlives the file buffer it was read from
        (!trimmed.is_empty()).then(|| Self(trimmed.to_owned()))
    }

    /// The key from `file`.
    ///
    /// # Errors
    /// Returns [`SkoposError::KeyFile`] if the file cannot be read.
    /// Returns [`SkoposError::NoApiKey`] if the file holds no key.
    pub fn from_file(file: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(file).map_err(|source| SkoposError::KeyFile {
            path: file.display().to_string(),
            source,
        })?;
        Self::new(&text).ok_or_else(|| SkoposError::NoApiKey {
            searched: file.display().to_string(),
        })
    }

    /// The key by the official CLI's search order (module docs).
    ///
    /// # Errors
    /// Returns [`SkoposError::NoApiKey`] if no source holds a key.
    /// Returns [`SkoposError::KeyFile`] if a key file exists but cannot be read.
    pub fn discover() -> Result<Self> {
        if let Some(key) = std::env::var("VAST_API_KEY")
            .ok()
            .as_deref()
            .and_then(Self::new)
        {
            return Ok(key);
        }
        let candidates = candidate_files();
        for file in &candidates {
            if file.is_file() {
                return Self::from_file(file);
            }
        }
        Err(SkoposError::NoApiKey {
            searched: candidates
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        })
    }

    /// The `Authorization` header value.
    pub(crate) fn bearer(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

/// The key files searched, in order (module docs, items 2-4).
#[must_use]
pub fn candidate_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        out.push(PathBuf::from(xdg).join("vastai").join("vast_api_key"));
    }
    if let Some(home) = home() {
        out.push(home.join(".config").join("vastai").join("vast_api_key"));
        out.push(home.join(".vast_api_key"));
    }
    out
}

/// The user's home: `HOME`, else `USERPROFILE` (Windows).
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
}

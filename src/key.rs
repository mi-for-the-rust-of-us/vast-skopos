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
        // A byte-order mark is not part of the key (Windows editors and PowerShell write one).
        let trimmed = text.trim().trim_start_matches('\u{feff}').trim();
        // BORROW: explicit to_owned -- the key outlives the file buffer it was read from
        (!trimmed.is_empty()).then(|| Self(trimmed.to_owned()))
    }

    /// The key from `file`.
    ///
    /// # Errors
    /// Returns [`SkoposError::KeyFile`] if the file cannot be read.
    /// Returns [`SkoposError::NoApiKey`] if the file holds no key.
    pub fn from_file(file: &Path) -> Result<Self> {
        let bytes = std::fs::read(file).map_err(|source| SkoposError::KeyFile {
            path: file.display().to_string(),
            source,
        })?;
        let text = decode(&bytes).ok_or_else(|| SkoposError::KeyFile {
            path: file.display().to_string(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "not UTF-8 or UTF-16 text (save the key as plain UTF-8)",
            ),
        })?;
        Self::new(&text).ok_or_else(|| SkoposError::NoApiKey {
            searched: file.display().to_string(),
            hint: String::new(),
        })
    }

    /// The key by the official CLI's search order (module docs).
    ///
    /// # Errors
    /// Returns [`SkoposError::NoApiKey`] if no source holds a key; when a searched file is missing
    /// but its `.txt` twin exists, the error names the twin ([`txt_hint`]).
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
            hint: txt_hint(&candidates, Path::is_file),
        })
    }

    /// The `Authorization` header value.
    pub(crate) fn bearer(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

/// A key (or config) file's text: UTF-16 with a byte-order mark (what Windows `PowerShell` 5.1's
/// `>` writes), else UTF-8; `None` when neither.
pub(crate) fn decode(bytes: &[u8]) -> Option<String> {
    let units = |big: bool| -> Vec<u16> {
        bytes
            .get(2..)
            .unwrap_or_default()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| {
                if big {
                    u16::from_be_bytes(pair)
                } else {
                    u16::from_le_bytes(pair)
                }
            })
            .collect()
    };
    match bytes {
        [0xFF, 0xFE, ..] => String::from_utf16(&units(false)).ok(),
        [0xFE, 0xFF, ..] => String::from_utf16(&units(true)).ok(),
        _ => String::from_utf8(bytes.to_vec()).ok(),
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

/// The note naming a missing key file's `.txt` twin; empty when no candidate has one.
///
/// Windows Notepad appends `.txt` to `vast_api_key`, so the key sits one rename away. `is_file`
/// is [`Path::is_file`] in use; it is a parameter so the rule can be tested without a filesystem.
///
/// ```
/// use std::path::{Path, PathBuf};
/// use vast_skopos::key::txt_hint;
/// let candidates = [PathBuf::from("/h/.config/vastai/vast_api_key")];
/// let hint = txt_hint(&candidates, |p| p == Path::new("/h/.config/vastai/vast_api_key.txt"));
/// assert!(hint.contains("vast_api_key.txt") && hint.contains("rename it to vast_api_key"));
/// assert_eq!(txt_hint(&candidates, |_| false), "");
/// ```
#[doc(hidden)]
#[must_use]
pub fn txt_hint(candidates: &[PathBuf], is_file: impl Fn(&Path) -> bool) -> String {
    candidates
        .iter()
        .find_map(|file| {
            let mut twin = file.clone().into_os_string();
            twin.push(".txt");
            let twin = PathBuf::from(twin);
            is_file(&twin).then(|| {
                let name = file
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                format!(
                    " ({} exists: Notepad appends .txt, so rename it to {name})",
                    twin.display()
                )
            })
        })
        .unwrap_or_default()
}

/// The user's home: `HOME`, else `USERPROFILE` (Windows).
pub(crate) fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
}

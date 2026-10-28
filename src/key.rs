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

/// The key in `text`: trimmed (a byte-order mark is not part of it: Windows editors and `PowerShell`
/// write one); `Ok(None)` when nothing is left; an error when what is left holds a space, a line
/// break or a control character (a second line in the file, or a UTF-16 file read as UTF-8, would
/// otherwise fail at the first request with a message about a header).
fn clean_key(text: &str) -> std::result::Result<Option<String>, &'static str> {
    let trimmed = text.trim().trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(
            "it holds a space, a line break or a control character (one line, plain UTF-8 or UTF-16 text)",
        );
    }
    // BORROW: explicit to_owned -- the key outlives the file buffer it was read from
    Ok(Some(trimmed.to_owned()))
}

impl ApiKey {
    /// A key from its text, trimmed; `None` when nothing is left, or when what is left holds a
    /// space, a line break or a control character (a key is one word).
    #[must_use]
    pub fn new(text: &str) -> Option<Self> {
        clean_key(text).ok().flatten().map(Self)
    }

    /// The key from `file`.
    ///
    /// # Errors
    /// Returns [`SkoposError::KeyFile`] if the file cannot be read.
    /// Returns [`SkoposError::NoApiKey`] if the file holds no key.
    pub fn from_file(file: &Path) -> Result<Self> {
        let text = read_text(file, "the key").map_err(|source| SkoposError::KeyFile {
            path: file.display().to_string(),
            source,
        })?;
        match clean_key(&text) {
            Ok(Some(key)) => Ok(Self(key)),
            Ok(None) => Err(SkoposError::NoApiKey {
                searched: file.display().to_string(),
                hint: String::from(" (that file is blank)"),
            }),
            Err(why) => Err(SkoposError::KeyFile {
                path: file.display().to_string(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("the key is not usable: {why}"),
                ),
            }),
        }
    }

    /// The key by the official CLI's search order (module docs).
    ///
    /// # Errors
    /// Returns [`SkoposError::NoApiKey`] if no source holds a key; when a searched file is missing
    /// but its `.txt` twin exists, the error names the twin (see `txt_hint`).
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

/// The text of the file at `path` (UTF-8, or UTF-16 with a byte-order mark); the error says what to
/// save as plain UTF-8 (`what`: "the key", "it").
pub(crate) fn read_text(path: &Path, what: &str) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    decode(&bytes).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("not UTF-8 or UTF-16 text (save {what} as plain UTF-8)"),
        )
    })
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
    // A UTF-16 file has an even number of bytes: an odd one is a truncated file, not text.
    // (`& 1`, not `.is_multiple_of(2)`, which the MSRV lacks.)
    let even = bytes.len() & 1 == 0;
    match bytes {
        [0xFF, 0xFE, ..] if even => String::from_utf16(&units(false)).ok(),
        [0xFE, 0xFF, ..] if even => String::from_utf16(&units(true)).ok(),
        [0xFF, 0xFE, ..] | [0xFE, 0xFF, ..] => None,
        _ => String::from_utf8(bytes.to_vec()).ok(),
    }
}

/// The directories config files live in, in order: `$XDG_CONFIG_HOME` when it is set, not empty
/// and absolute (the XDG specification says to ignore a relative one: it would make the files
/// searched depend on the folder `vas` runs from), then `~/.config`.
pub(crate) fn config_dirs() -> Vec<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute());
    xdg.into_iter()
        .chain(home().map(|h| h.join(".config")))
        .collect()
}

/// The key files searched, in order (module docs, items 2-4).
#[must_use]
pub fn candidate_files() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = config_dirs()
        .into_iter()
        .map(|dir| dir.join("vastai").join("vast_api_key"))
        .collect();
    if let Some(home) = home() {
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
// Hidden, not promised: the wording of `vas`'s own error, public only so the integration tests
// can drive it without a filesystem.
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

/// The user's home, as the official CLI finds it (Python's `expanduser`): on Windows `USERPROFILE`
/// first, since `HOME` there is often an MSYS or Cygwin path (`/c/Users/...`) that no Windows API
/// understands; elsewhere `HOME`. The other is the fallback.
pub(crate) fn home() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let (first, second) = if cfg!(windows) {
        ("USERPROFILE", "HOME")
    } else {
        ("HOME", "USERPROFILE")
    };
    var(first).or_else(|| var(second)).map(PathBuf::from)
}

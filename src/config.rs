//! The `vas offers` config file: flags read before the command line's.
//!
//! One flag per line, written as on the command line: `--gpu RTX_5090`, `--gpu="RTX 5090"`,
//! `--verified`. Blank lines and lines starting with `#` are skipped; one pair of quotes around a
//! value is removed. The file's flags come FIRST and the command line's after, so a single value
//! given in both places is the command line's, while the deny-lists (`--exclude-country`,
//! `--exclude-host`) accumulate: adding one entry on the command line never drops the file's.
//!
//! The file is the one `--config FILE` names, else the first that exists of:
//!
//! 1. `$XDG_CONFIG_HOME/vast-skopos/offers`, when `XDG_CONFIG_HOME` is set;
//! 2. `~/.config/vast-skopos/offers`.
//!
//! No default file is no file. But a `--config` file that is missing, a file that cannot be read,
//! and a line that is not a flag all STOP the run: going on without them would silently drop the
//! exclusions they hold.

use std::path::{Path, PathBuf};

use crate::error::{Result, SkoposError};
use crate::key::{decode, home};

/// The default config files, in order (module docs).
#[must_use]
pub fn offers_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        out.push(PathBuf::from(xdg).join("vast-skopos").join("offers"));
    }
    if let Some(home) = home() {
        out.push(home.join(".config").join("vast-skopos").join("offers"));
    }
    out
}

/// The default config file: the first of [`offers_files`] that exists.
#[must_use]
pub fn default_offers_file() -> Option<PathBuf> {
    offers_files().into_iter().find(|p| p.is_file())
}

/// `value` without one pair of surrounding quotes; an error when a quote opens and does not close.
fn unquote(value: &str) -> std::result::Result<&str, String> {
    for quote in ['"', '\''] {
        if let Some(inner) = value.strip_prefix(quote) {
            return inner
                .strip_suffix(quote)
                .ok_or_else(|| format!("the {quote} opening the value does not close it"));
        }
    }
    Ok(value)
}

/// One line's command-line word (`--name` or `--name=value`); an error naming what is wrong.
fn flag_word(line: &str) -> std::result::Result<String, String> {
    let rest = line.strip_prefix("--").ok_or_else(|| {
        String::from("not a flag: each line is `--flag` or `--flag value` (or `# a comment`)")
    })?;
    let (name, value) = match rest.find(|c: char| c == '=' || c.is_whitespace()) {
        None => (rest, None),
        Some(at) => {
            let (name, tail) = rest.split_at(at);
            let value = if let Some(after) = tail.strip_prefix('=') {
                after.trim()
            } else {
                let after = tail.trim_start();
                if after.starts_with('=') {
                    return Err(String::from(
                        "write `--flag value` or `--flag=value`, not both",
                    ));
                }
                after
            };
            (name, Some(value))
        }
    };
    if name.is_empty()
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(format!("`--{name}` is not a flag name"));
    }
    if name == "config" || name == "no-config" {
        return Err(format!(
            "`--{name}` cannot be set from a config file (it chooses the file)"
        ));
    }
    let Some(value) = value else {
        return Ok(format!("--{name}"));
    };
    if value.contains('#') {
        return Err(String::from(
            "a `#` inside a value: comments go on lines of their own",
        ));
    }
    let value = unquote(value)?;
    if value.trim().is_empty() {
        return Err(format!("`--{name}` has an empty value"));
    }
    // One word, `--name=value`: clap then reads the value as a value even when it starts with `-`.
    Ok(format!("--{name}={value}"))
}

/// The command-line words of a config file's text (module docs), one per flag line; `path` only
/// names the file in errors.
///
/// ```
/// use std::path::Path;
/// use vast_skopos::config::parse_flag_file;
/// let file = Path::new("offers");
/// let text = "# askesis's renting filters\n--gpu RTX_5090\r\n\n--reliability=0.99\n\
///             --exclude-country \"CN\"\n  --verified  \n";
/// assert_eq!(parse_flag_file(text, file).unwrap(),
///   ["--gpu=RTX_5090", "--reliability=0.99", "--exclude-country=CN", "--verified"]);
/// assert_eq!(parse_flag_file("--gpu 'RTX 5090'", file).unwrap(), ["--gpu=RTX 5090"]);
/// let err = parse_flag_file("--top 5\n--gpu RTX_5090 # the card\n", file).unwrap_err();
/// assert_eq!(err.to_string(),
///   "offers, line 2: a `#` inside a value: comments go on lines of their own");
/// for bad in ["gpu RTX_5090", "--", "--GPU x", "--gpu \"RTX", "--gpu = x", "--gpu ''",
///             "--config other", "--no-config"] {
///     assert!(parse_flag_file(bad, file).is_err(), "{bad}");
/// }
/// ```
///
/// # Errors
/// Returns [`SkoposError::Config`] naming the first line that is not a flag.
pub fn parse_flag_file(text: &str, path: &Path) -> Result<Vec<String>> {
    let mut words = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        words.push(flag_word(line).map_err(|problem| SkoposError::Config {
            path: path.display().to_string(),
            line: n + 1,
            problem,
        })?);
    }
    Ok(words)
}

/// The command-line words of the config file at `path` (UTF-8, or UTF-16 with a byte-order mark).
///
/// # Errors
/// Returns [`SkoposError::ConfigFile`] if the file cannot be read or is not UTF-8 or UTF-16 text.
/// Returns [`SkoposError::Config`] naming the first line that is not a flag.
pub fn read_flag_file(path: &Path) -> Result<Vec<String>> {
    let bytes = std::fs::read(path).map_err(|source| SkoposError::ConfigFile {
        path: path.display().to_string(),
        source,
    })?;
    let text = decode(&bytes).ok_or_else(|| SkoposError::ConfigFile {
        path: path.display().to_string(),
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "not UTF-8 or UTF-16 text (save it as plain UTF-8)",
        ),
    })?;
    // A byte-order mark is not part of the first line (Windows editors write one).
    parse_flag_file(text.trim_start_matches('\u{feff}'), path)
}

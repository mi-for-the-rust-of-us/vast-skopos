//! The credit history: one CSV row per look, appended to a file the user names.
//!
//! The account's credit is only a number today; with one row per look it becomes a curve, and the
//! burn beside it says whether the curve matches the boxes. The burn counts the boxes meant to run
//! that read otherwise (`unsure`), so compare the fall of the credit with `burn_per_hour`: a fall
//! well below it, while `unsure` is above zero, says those boxes do not bill; a fall above it says
//! something else does (vast.ai bills the storage of a stopped box, which `vas` does not count). This
//! is the one file the crate writes, and only when asked (`vas --credit-log FILE`): see
//! CONVENTIONS.md, "When Writing a File".
//!
//! ```text
//! unix_time,credit,burn_per_hour,running,unsure
//! 1792328430,10.5100,0.4910,1,0
//! ```

use std::fs::OpenOptions;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use crate::instance::whole_secs;
use crate::runway::{Runway, dollars};

/// The first line of a new log: the columns of [`CreditLog::row`].
pub const HEADER: &str = "unix_time,credit,burn_per_hour,running,unsure";

/// A credit log: the file rows are appended to.
///
/// The first failure to write is returned once, as a message; after it the log writes nothing more
/// (a full disk or a wrong path is not worth a line per look), and the look goes on.
///
/// ```
/// use vast_skopos::{creditlog::CreditLog, instance::{Account, Instance}, runway::Runway};
/// let dir = std::env::temp_dir().join(format!("vas-doc-creditlog-{}", std::process::id()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let file = dir.join("credit.csv");
/// let running = Instance::from_json(&serde_json::json!(
///   {"id": 1, "actual_status": "running", "dph_total": 0.5})).unwrap();
/// let account = Account::from_json(&serde_json::json!({"credit": 12.059765171599793}));
/// let runway = Runway::of(&[running], Some(&account));
/// let mut log = CreditLog::new(&file);
/// assert_eq!(log.record(&runway, 1_792_328_430.9), None);
/// assert_eq!(std::fs::read_to_string(&file).unwrap(),
///   "unix_time,credit,burn_per_hour,running,unsure\n1792328430,12.0598,0.5000,1,0\n");
/// std::fs::remove_dir_all(&dir).unwrap();
/// ```
#[derive(Debug)]
pub struct CreditLog {
    /// The file the rows are appended to.
    path: PathBuf,
    /// Whether a write failed: the log is then silent.
    failed: bool,
}

impl CreditLog {
    /// A log appending to `path`; nothing is created until the first row.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            failed: false,
        }
    }

    /// The file the rows go to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// One row, ending in a newline: the Unix time in whole seconds, the credit (empty when the
    /// account did not answer) and the burn in dollars to ten-thousandths, the running boxes, and
    /// the boxes meant to run that read otherwise.
    ///
    /// ```
    /// use vast_skopos::{creditlog::CreditLog, instance::Account, runway::Runway};
    /// let a = Account::from_json(&serde_json::json!({"credit": 5.0}));
    /// assert_eq!(CreditLog::row(&Runway::of(&[], Some(&a)), 7.5), "7,5.0000,0.0000,0,0\n");
    /// assert_eq!(CreditLog::row(&Runway::of(&[], None), 7.5), "7,,0.0000,0,0\n", "no credit, no number");
    /// ```
    #[must_use]
    pub fn row(runway: &Runway, at: f64) -> String {
        let credit = runway.credit.map_or_else(String::new, |c| dollars(c, 4));
        let unsure: usize = runway.unsure.values().sum();
        format!(
            "{},{credit},{},{},{unsure}\n",
            whole_secs(at),
            dollars(runway.burn, 4),
            runway.running
        )
    }

    /// Appends the row of this look, with the header first when the file is new or empty.
    ///
    /// Returns the message of the first failure (`failed to write the credit log FILE: ...`), once;
    /// after a failure nothing more is written and `None` is returned. A row is appended on a line of
    /// its own, even to a file whose last line has no newline.
    #[must_use = "the message is the only report of a failure to write"]
    pub fn record(&mut self, runway: &Runway, at: f64) -> Option<String> {
        if self.failed {
            return None;
        }
        match self.append(&Self::row(runway, at)) {
            Ok(()) => None,
            Err(e) => {
                self.failed = true;
                Some(format!(
                    "failed to write the credit log {}: {e}",
                    self.path.display()
                ))
            }
        }
    }

    /// Appends `row`, preceded by the header when the file is empty, in one write.
    fn append(&self, row: &str) -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .read(true)
            .create(true)
            .append(true)
            .open(&self.path)?;
        let len = file.metadata()?.len();
        let mut text = String::new();
        if len == 0 {
            text.push_str(HEADER);
            text.push('\n');
        } else if !ends_with_newline(&mut file)? {
            // A file edited by hand may end mid-line: the row must not glue onto it.
            text.push('\n');
        }
        text.push_str(row);
        file.write_all(text.as_bytes())
    }
}

/// Whether the last byte of the (non-empty) `file` is a newline.
fn ends_with_newline(file: &mut std::fs::File) -> std::io::Result<bool> {
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0_u8; 1];
    file.read_exact(&mut last)?;
    Ok(last == *b"\n")
}

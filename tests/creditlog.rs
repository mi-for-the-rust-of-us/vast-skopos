//! The credit log: its rows, its header, and what it does when the file cannot be written.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

use std::path::PathBuf;

use vast_skopos::creditlog::{CreditLog, HEADER};
use vast_skopos::instance::{Account, Instance};
use vast_skopos::runway::Runway;

/// A fresh directory, removed when the test ends, even when an assertion panics first.
struct Dir(PathBuf);

impl std::ops::Deref for Dir {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("vas-creditlog-lib-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

fn instance(id: u64, actual: &str, dph: f64) -> Instance {
    Instance::from_json(&serde_json::json!(
        {"id": id, "actual_status": actual, "intended_status": "running", "dph_total": dph}))
    .unwrap()
}

fn account(credit: f64) -> Account {
    Account::from_json(&serde_json::json!({"credit": credit}))
}

#[test]
fn a_row_is_the_time_the_credit_the_burn_the_running_and_the_unsure() {
    let boxes = [
        instance(1, "running", 0.5),
        instance(2, "running", 0.25),
        instance(3, "offline", 0.1),
        instance(4, "loading", 0.1),
        instance(5, "offline", 0.1),
    ];
    let runway = Runway::of(&boxes, Some(&account(12.059_765_171_599_793)));
    assert_eq!(
        CreditLog::row(&runway, 1_792_328_430.9),
        "1792328430,12.0598,1.0500,2,3\n",
        "whole seconds, four decimals, the unsure counted whatever their state"
    );
    // Without the account's answer there is no credit to write, and none is made up.
    let blind = Runway::of(&boxes, None);
    assert_eq!(CreditLog::row(&blind, 5.0), "5,,1.0500,2,3\n");
}

#[test]
fn a_credit_that_rounds_to_zero_or_a_debt_is_written_without_a_negative_zero() {
    let none: [Instance; 0] = [];
    assert_eq!(
        CreditLog::row(&Runway::of(&none, Some(&account(-0.000_01))), 1.0),
        "1,0.0000,0.0000,0,0\n"
    );
    assert_eq!(
        CreditLog::row(&Runway::of(&none, Some(&account(-5.0))), 1.0),
        "1,-5.0000,0.0000,0,0\n"
    );
}

#[test]
fn the_header_is_written_once_to_a_new_or_empty_file_and_never_again() {
    let dir = dir("header");
    let runway = Runway::of(&[instance(1, "running", 0.5)], Some(&account(5.0)));
    // A new file.
    let new = dir.join("new.csv");
    let mut log = CreditLog::new(&new);
    assert_eq!(log.path(), new);
    assert!(!new.exists(), "nothing is created before the first row");
    assert_eq!(log.record(&runway, 1.0), None);
    assert_eq!(log.record(&runway, 2.0), None);
    // A second run (another CreditLog) on the same file appends.
    assert_eq!(CreditLog::new(&new).record(&runway, 3.0), None);
    let text = std::fs::read_to_string(&new).unwrap();
    assert_eq!(
        text,
        format!("{HEADER}\n1,5.0000,0.5000,1,0\n2,5.0000,0.5000,1,0\n3,5.0000,0.5000,1,0\n")
    );
    // An empty file that exists gets the header too.
    let empty = dir.join("empty.csv");
    std::fs::write(&empty, "").unwrap();
    assert_eq!(CreditLog::new(&empty).record(&runway, 1.0), None);
    assert!(std::fs::read_to_string(&empty).unwrap().starts_with(HEADER));
    // A file whose last line has no newline: the row starts a line of its own.
    let glued = dir.join("glued.csv");
    std::fs::write(&glued, "my notes").unwrap();
    assert_eq!(CreditLog::new(&glued).record(&runway, 1.0), None);
    assert_eq!(
        std::fs::read_to_string(&glued).unwrap(),
        "my notes\n1,5.0000,0.5000,1,0\n"
    );
    // A file with content is appended to, whatever it holds, and its content is kept.
    let mine = dir.join("mine.csv");
    std::fs::write(&mine, "my notes\n").unwrap();
    assert_eq!(CreditLog::new(&mine).record(&runway, 1.0), None);
    assert_eq!(
        std::fs::read_to_string(&mine).unwrap(),
        "my notes\n1,5.0000,0.5000,1,0\n"
    );
}

#[test]
fn a_file_that_cannot_be_written_is_said_once_then_left_alone() {
    let dir = dir("refused");
    let runway = Runway::of(&[], Some(&account(5.0)));
    // A directory is not a file one can append to.
    let mut log = CreditLog::new(dir.clone());
    let message = log.record(&runway, 1.0).unwrap();
    assert!(
        message.starts_with("failed to write the credit log "),
        "{message}"
    );
    assert!(message.contains(&dir.display().to_string()), "{message}");
    assert_eq!(log.record(&runway, 2.0), None, "said once");
    // After a failure the log does not try again; a new log on a good path does.
    let mut log = CreditLog::new(dir.join("missing").join("credit.csv"));
    assert!(log.record(&runway, 1.0).is_some(), "no such directory");
    assert_eq!(log.record(&runway, 2.0), None);
    let repaired = dir.join("later.csv");
    assert_eq!(CreditLog::new(&repaired).record(&runway, 1.0), None);
    assert!(repaired.exists());
}

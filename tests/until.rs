//! What a look says to `vas --until`: a match, something still to wait on, or nothing.

#![allow(clippy::unwrap_used, clippy::unnecessary_wraps)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion; and the helpers return the
// `ProbeOutcome` (an `Option`) that `verdict` takes.

use vast_skopos::SkoposError;
use vast_skopos::instance::Instance;
use vast_skopos::probe::{ProbeOutcome, parse_output};
use vast_skopos::until::{Verdict, verdict};

fn instance(id: u64, actual: &str, intended: &str) -> Instance {
    Instance::from_json(
        &serde_json::json!({"id": id, "actual_status": actual, "intended_status": intended}),
    )
    .unwrap()
}

/// A probe whose log half is `log` (what follows the separator).
fn read(log: &str) -> ProbeOutcome {
    Some(Ok(parse_output(&format!(
        "0, 99, 1, 2\n---vas-log---\n{log}"
    ))))
}

fn matched(line: &str) -> ProbeOutcome {
    read(&format!(
        "vas-grep-file:/r/run.log\n{line}\nvas-grep-exit:0\n"
    ))
}

fn not_yet() -> ProbeOutcome {
    read("vas-grep-file:/r/run.log\nvas-grep-exit:1\n")
}

fn refused() -> ProbeOutcome {
    Some(Err(SkoposError::Ssh {
        target: "h:1".to_owned(),
        message: "Connection refused".to_owned(),
    }))
}

#[test]
fn the_first_box_whose_log_matches_ends_the_wait() {
    let boxes = [
        instance(1, "running", "running"),
        instance(2, "running", "running"),
        instance(3, "running", "running"),
    ];
    let probes = [refused(), matched("initial loss 4.21"), matched("loss 9")];
    assert_eq!(
        verdict(&boxes, &probes),
        Verdict::Matched {
            id: 2,
            line: "initial loss 4.21".to_owned()
        }
    );
}

#[test]
fn a_box_read_without_a_match_yet_keeps_the_wait_whatever_its_file_says() {
    let one = [instance(1, "running", "running")];
    assert_eq!(verdict(&one, &[not_yet()]), Verdict::Waiting);
    assert_eq!(
        verdict(&one, &[read("vas-no-file\n")]),
        Verdict::Waiting,
        "the file may not be written yet"
    );
    assert_eq!(
        verdict(&one, &[read("vas-grep-file:/r/run.log\nvas-grep-exit:2\n")]),
        Verdict::Waiting,
        "a refused pattern is said in the table on every look; the box was read"
    );
}

#[test]
fn a_box_still_starting_keeps_the_wait_and_nothing_to_read_ends_it() {
    let loading = instance(1, "loading", "running");
    let gone = instance(2, "exited", "stopped");
    let failing = instance(3, "running", "running");
    assert_eq!(
        verdict(std::slice::from_ref(&loading), &[None]),
        Verdict::Waiting,
        "a box rented a minute ago does not count against the wait"
    );
    assert_eq!(
        verdict(&[gone.clone(), failing.clone()], &[None, refused()]),
        Verdict::Unread
    );
    assert_eq!(verdict(&[], &[]), Verdict::Unread, "no box at all");
    assert_eq!(
        verdict(&[loading, failing], &[None, refused()]),
        Verdict::Waiting
    );
    assert_eq!(
        verdict(&[gone], &[]),
        Verdict::Unread,
        "fewer probes than boxes read as not probed"
    );
}

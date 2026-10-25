//! What a look says to `vas --until` (a match, a refused pattern, a box read, a box starting, or
//! nothing), and when a wait across looks ends.

#![allow(clippy::unwrap_used, clippy::unnecessary_wraps)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion; and the helpers return the
// `ProbeOutcome` (an `Option`) that `verdict` takes.

use vast_skopos::SkoposError;
use vast_skopos::instance::Instance;
use vast_skopos::probe::{ProbeOutcome, parse_output};
use vast_skopos::until::{End, STARTING_LOOKS, UNREAD_LOOKS, Verdict, Wait, verdict};

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

fn grep_refused() -> ProbeOutcome {
    read("vas-grep-file:/r/run.log\nvas-grep-exit:2\n")
}

fn ssh_refused() -> ProbeOutcome {
    Some(Err(SkoposError::Ssh {
        target: "h:1".to_owned(),
        message: "Connection refused".to_owned(),
    }))
}

#[test]
fn the_first_box_whose_log_matches_ends_the_wait_even_beside_a_refused_pattern() {
    let boxes = [
        instance(1, "running", "running"),
        instance(2, "running", "running"),
        instance(3, "running", "running"),
    ];
    let probes = [
        grep_refused(),
        matched("initial loss 4.21"),
        matched("loss 9"),
    ];
    assert_eq!(
        verdict(&boxes, &probes),
        Verdict::Matched {
            id: 2,
            line: "initial loss 4.21".to_owned()
        }
    );
    let probes = [ssh_refused(), grep_refused(), None];
    assert_eq!(verdict(&boxes, &probes), Verdict::Refused { id: 2 });
    let probes = [ssh_refused(), not_yet(), grep_refused()];
    assert_eq!(
        verdict(&boxes, &probes),
        Verdict::Read,
        "box 2 took the pattern and may still match: one box's refusal does not end the wait"
    );
}

#[test]
fn a_log_the_box_could_not_give_is_no_box_read() {
    let one = [instance(1, "running", "running")];
    for cut in [
        "vas-grep-file:/r/run.log\nvas-grep-exit:124\n",
        "vas-grep-file:/r/run.log\nvas-grep-exit:3\n",
        "vas-grep-file:/r/run.log\n",
    ] {
        assert_eq!(verdict(&one, &[read(cut)]), Verdict::Unread, "{cut:?}");
    }
}

#[test]
fn a_box_read_without_a_match_yet_is_read_whatever_its_file_says() {
    let one = [instance(1, "running", "running")];
    assert_eq!(verdict(&one, &[not_yet()]), Verdict::Read);
    assert_eq!(
        verdict(&one, &[read("vas-no-file\n")]),
        Verdict::Read,
        "the file may not be written yet"
    );
    assert_eq!(verdict(&one, &[read("vas-not-a-file:/r\n")]), Verdict::Read);
}

#[test]
fn a_box_not_running_is_starting_and_nothing_to_read_is_unread() {
    let loading = instance(1, "loading", "running");
    let gone = instance(2, "exited", "stopped");
    let failing = instance(3, "running", "running");
    assert_eq!(
        verdict(std::slice::from_ref(&loading), &[None]),
        Verdict::Starting
    );
    assert_eq!(
        verdict(&[gone, failing.clone()], &[None, ssh_refused()]),
        Verdict::Unread
    );
    assert_eq!(verdict(&[], &[]), Verdict::Unread, "no box at all");
    assert_eq!(
        verdict(&[loading, failing.clone()], &[None, ssh_refused()]),
        Verdict::Starting
    );
    assert_eq!(
        verdict(&[failing], &[]),
        Verdict::Unread,
        "fewer probes than boxes read as not probed"
    );
}

#[test]
fn a_box_read_between_two_unread_looks_starts_the_count_afresh() {
    let mut wait = Wait::default();
    for _ in 0..5 {
        assert_eq!(wait.record(Verdict::Unread), None);
        assert_eq!(wait.record(Verdict::Read), None);
    }
    for n in 1..UNREAD_LOOKS {
        assert_eq!(wait.record(Verdict::Unread), None, "look {n}");
    }
    assert_eq!(wait.record(Verdict::Unread), Some(End::GaveUp));
}

#[test]
fn a_box_that_stays_not_running_keeps_a_wait_for_a_bounded_number_of_looks() {
    let mut wait = Wait::default();
    for n in 0..STARTING_LOOKS {
        assert_eq!(wait.record(Verdict::Starting), None, "look {n}");
    }
    // A box read resets the bound: it may start again later.
    assert_eq!(wait.record(Verdict::Read), None);
    for n in 0..STARTING_LOOKS {
        assert_eq!(
            wait.record(Verdict::Starting),
            None,
            "look {n} after the read"
        );
    }
    let ends: Vec<Option<End>> = (0..UNREAD_LOOKS)
        .map(|_| wait.record(Verdict::Starting))
        .collect();
    assert_eq!(
        ends.last(),
        Some(&Some(End::GaveUp)),
        "past the bound, a starting look counts as unread: {ends:?}"
    );
}

#[test]
fn a_match_and_a_refused_pattern_end_the_wait_at_once() {
    assert_eq!(
        Wait::default().record(Verdict::Matched {
            id: 7,
            line: "x".to_owned()
        }),
        Some(End::Matched {
            id: 7,
            line: "x".to_owned()
        })
    );
    assert_eq!(
        Wait::default().record(Verdict::Refused { id: 7 }),
        Some(End::Refused { id: 7 })
    );
}

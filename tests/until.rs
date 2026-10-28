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
fn a_box_read_without_a_match_yet_is_read_and_one_without_the_file_is_not() {
    let one = [instance(1, "running", "running")];
    assert_eq!(verdict(&one, &[not_yet()]), Verdict::Read);
    assert_eq!(
        verdict(&one, &[read("vas-no-file\n")]),
        Verdict::NoFile,
        "no file matches the path: not written yet, or the path is wrong"
    );
    assert_eq!(
        verdict(&one, &[read("vas-not-a-file:/r\n")]),
        Verdict::NoFile
    );
    // An exact path that exists on one box of two (seed 1's log on box 2): the wait goes on.
    let two = [
        instance(1, "running", "running"),
        instance(2, "running", "running"),
    ];
    assert_eq!(
        verdict(&two, &[read("vas-no-file\n"), not_yet()]),
        Verdict::Read
    );
}

#[test]
fn a_path_no_box_has_a_file_for_warns_once_then_ends_the_wait_on_the_clock() {
    use vast_skopos::until::{NO_FILE_SECS, NO_FILE_WARN_LOOKS};
    // Looks a minute apart: the wait ends at the first look NO_FILE_SECS past the first one.
    let mut wait = Wait::default();
    let start = 1_000_000;
    let mut warned = Vec::new();
    let mut at = start;
    while at < start + NO_FILE_SECS {
        assert_eq!(wait.record_at(Verdict::NoFile, at), None, "at {at}");
        warned.push(wait.warns_of_no_file());
        at += 60;
    }
    assert_eq!(wait.record_at(Verdict::NoFile, at), Some(End::NoFile));
    let warns: Vec<usize> = warned
        .iter()
        .enumerate()
        .filter(|(_, w)| **w)
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        warns,
        [usize::try_from(NO_FILE_WARN_LOOKS).unwrap() - 1],
        "warned once, at look {NO_FILE_WARN_LOOKS}"
    );
    // The time is the wait's: one second short does not end it.
    let mut wait = Wait::default().no_file_secs(120);
    assert_eq!(wait.record_at(Verdict::NoFile, 10), None);
    assert_eq!(wait.record_at(Verdict::NoFile, 129), None);
    assert_eq!(wait.record_at(Verdict::NoFile, 130), Some(End::NoFile));
    // A clock that goes back ends nothing.
    let mut wait = Wait::default().no_file_secs(1);
    assert_eq!(wait.record_at(Verdict::NoFile, 500), None);
    assert_eq!(wait.record_at(Verdict::NoFile, 400), None);
}

#[test]
fn a_file_appearing_starts_the_time_afresh_and_keeps_the_wait_s_own() {
    let mut wait = Wait::default().no_file_secs(120);
    assert_eq!(wait.record_at(Verdict::NoFile, 0), None);
    assert_eq!(wait.record_at(Verdict::NoFile, 100), None);
    assert_eq!(wait.record_at(Verdict::Read, 110), None, "a file appeared");
    assert_eq!(
        wait.record_at(Verdict::NoFile, 200),
        None,
        "the time starts here"
    );
    assert_eq!(wait.record_at(Verdict::NoFile, 319), None);
    assert_eq!(
        wait.record_at(Verdict::NoFile, 320),
        Some(End::NoFile),
        "120 s still, not the default"
    );
}

#[test]
fn looks_with_nothing_to_read_neither_restart_the_time_nor_warn_again() {
    let mut wait = Wait::default().no_file_secs(120);
    let mut warned = Vec::new();
    for (verdict, at) in [
        (Verdict::NoFile, 0),
        (Verdict::NoFile, 30),
        (Verdict::Unread, 60),
        (Verdict::NoFile, 90),
    ] {
        assert_eq!(wait.record_at(verdict, at), None);
        warned.push(wait.warns_of_no_file());
    }
    assert_eq!(warned, [false, true, false, false], "warned once");
    assert_eq!(
        wait.record_at(Verdict::NoFile, 120),
        Some(End::NoFile),
        "counted from the first look without the file"
    );
    // A look of boxes read without the file is no look of nothing to read: ssh works.
    let mut wait = Wait::default();
    assert_eq!(wait.record_at(Verdict::Unread, 0), None);
    assert_eq!(wait.record_at(Verdict::NoFile, 10), None);
    assert_eq!(
        wait.record_at(Verdict::Unread, 20),
        None,
        "the unread count started afresh"
    );
}

#[test]
fn a_box_without_the_file_runs_the_clock_whatever_the_other_boxes_say() {
    let running = instance(1, "running", "running");
    let other = instance(2, "running", "running");
    let loading = instance(3, "loading", "running");
    let no_file = || read("vas-no-file\n");
    let cut = || read("vas-grep-file:/r/run.log\nvas-grep-exit:124\n");
    let looks: [(&str, Vec<Instance>, Vec<ProbeOutcome>); 4] = [
        (
            "box 2's log was cut short",
            vec![running.clone(), other.clone()],
            vec![no_file(), cut()],
        ),
        (
            "box 3 is loading",
            vec![running.clone(), loading],
            vec![no_file(), None],
        ),
        (
            "box 2's ssh failed",
            vec![running.clone(), other.clone()],
            vec![no_file(), ssh_refused()],
        ),
        (
            "box 2's grep refused the pattern: box 1's may take it once its file is written",
            vec![running, other],
            vec![no_file(), grep_refused()],
        ),
    ];
    for (why, boxes, probes) in looks {
        assert_eq!(verdict(&boxes, &probes), Verdict::NoFile, "{why}");
        // Such looks a minute apart end the wait at --log-wait, never sooner (the 2026-10-28 review:
        // two of them once gave up at the second look, "no box could be read").
        let mut wait = Wait::default().no_file_secs(600);
        for n in 0..10 {
            assert_eq!(
                wait.record_at(verdict(&boxes, &probes), n * 60),
                None,
                "{why}: look {n}"
            );
        }
        assert_eq!(
            wait.record_at(verdict(&boxes, &probes), 600),
            Some(End::NoFile),
            "{why}"
        );
    }
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

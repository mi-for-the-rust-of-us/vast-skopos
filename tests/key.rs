//! The missing-key error, with and without the `.txt` hint.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

use std::path::{Path, PathBuf};

use vast_skopos::SkoposError;
use vast_skopos::key::txt_hint;

#[test]
fn no_api_key_reads_well_with_and_without_the_hint() {
    let plain = SkoposError::NoApiKey {
        searched: "/h/a, /h/b".to_owned(),
        hint: String::new(),
    }
    .to_string();
    assert_eq!(
        plain,
        "no vast.ai API key found (set VAST_API_KEY or write it to one of: /h/a, /h/b)"
    );

    let candidates = [PathBuf::from("/h/vast_api_key")];
    let hint = txt_hint(&candidates, |p| p == Path::new("/h/vast_api_key.txt"));
    let with = SkoposError::NoApiKey {
        searched: "/h/vast_api_key".to_owned(),
        hint,
    }
    .to_string();
    assert!(
        with.ends_with(
            "(/h/vast_api_key.txt exists: Notepad appends .txt, so rename it to vast_api_key)"
        ),
        "{with}"
    );
    assert!(!with.ends_with('.'), "no trailing period: {with}");
}

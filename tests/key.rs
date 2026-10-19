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
use vast_skopos::key::{ApiKey, txt_hint};

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

/// A scratch file under the temp dir with these bytes.
fn file_with(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vas-keytest-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("vast_api_key");
    std::fs::write(&file, bytes).unwrap();
    file
}

#[test]
fn a_key_is_one_word_and_anything_else_is_refused_with_a_message_that_does_not_show_it() {
    assert!(ApiKey::new("  k3y123  ").is_some());
    assert!(
        ApiKey::new("\u{feff}k3y123\r\n").is_some(),
        "a byte-order mark and a line break around it"
    );
    for bad in [
        "k3y 123",
        "k3y\n123",
        "k3y\t123",
        "k3y\u{0}123",
        "k3y\u{7f}",
    ] {
        assert!(ApiKey::new(bad).is_none(), "{bad:?}");
    }
    assert!(ApiKey::new("   ").is_none());
    // From a file: a second line is an error that says what is wrong, not a missing key and not a
    // failure about a header at the first request, and that never prints the key.
    let file = file_with("twolines", b"SECRETKEY123\nsecond line\n");
    let err = ApiKey::from_file(&file).unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, SkoposError::KeyFile { .. }), "{text}");
    assert!(
        text.contains("not usable") && text.contains("one line"),
        "{text}"
    );
    assert!(!text.contains("SECRETKEY123"), "{text}");
    // A blank file is still a missing key.
    let blank = file_with("blank", b"  \r\n");
    assert!(matches!(
        ApiKey::from_file(&blank).unwrap_err(),
        SkoposError::NoApiKey { .. }
    ));
    // A good one.
    let good = file_with("good", b"k3y123\r\n");
    assert!(ApiKey::from_file(&good).is_ok());
    for f in [file, blank, good] {
        let _ = std::fs::remove_dir_all(f.parent().unwrap());
    }
}

#[test]
fn a_utf16_key_file_is_read_whole_or_refused_never_half_read() {
    let utf16 = |text: &str, extra: &[u8]| -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(extra);
        bytes
    };
    let whole = file_with("utf16", &utf16("k3y123\r\n", &[]));
    assert!(
        ApiKey::from_file(&whole).is_ok(),
        "what PowerShell 5.1's > writes"
    );
    // A truncated file: one byte of the last unit. It was read as a shorter, valid key.
    let cut = file_with("utf16cut", &utf16("k3y123", &[0x41]));
    let err = ApiKey::from_file(&cut).unwrap_err();
    assert!(matches!(err, SkoposError::KeyFile { .. }), "{err}");
    assert!(err.to_string().contains("UTF-16"), "{err}");
    for f in [whole, cut] {
        let _ = std::fs::remove_dir_all(f.parent().unwrap());
    }
}

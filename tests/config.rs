//! The `vas offers` config file, read from disk: encodings, a missing file, a malformed line.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

use vast_skopos::SkoposError;
use vast_skopos::config::read_flag_file;

#[test]
fn a_config_file_reads_the_same_in_utf8_with_a_bom_and_in_utf16() {
    let dir = std::env::temp_dir().join(format!("vas-config-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let text = "# filters\r\n--gpu RTX_5090\r\n--exclude-host 362355\r\n";
    let bom = dir.join("bom");
    std::fs::write(&bom, format!("\u{feff}{text}")).unwrap();
    let utf16 = dir.join("utf16");
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&utf16, bytes).unwrap();
    // Big-endian UTF-16 (a byte-order mark FE FF): the same words, not mojibake.
    let utf16_be = dir.join("utf16-be");
    let mut bytes = vec![0xFE, 0xFF];
    bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    std::fs::write(&utf16_be, bytes).unwrap();
    let binary = dir.join("binary");
    std::fs::write(&binary, [0xC3, 0x28, 0x0A]).unwrap();
    let words = [
        read_flag_file(&bom),
        read_flag_file(&utf16),
        read_flag_file(&utf16_be),
    ];
    let not_text = read_flag_file(&binary);
    let missing = read_flag_file(&dir.join("missing"));
    std::fs::remove_dir_all(&dir).unwrap();
    for w in words {
        assert_eq!(w.unwrap(), ["--gpu=RTX_5090", "--exclude-host=362355"]);
    }
    assert!(matches!(not_text, Err(SkoposError::ConfigFile { .. })));
    assert!(matches!(missing, Err(SkoposError::ConfigFile { .. })));
}

#[test]
fn a_malformed_line_names_the_file_and_the_line() {
    let dir = std::env::temp_dir().join(format!("vas-config-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("offers");
    std::fs::write(&file, "--gpu RTX_5090\n\ngpu RTX_4090\n").unwrap();
    let err = read_flag_file(&file).unwrap_err();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        matches!(err, SkoposError::ConfigLine { line: 3, .. }),
        "{err}"
    );
    let text = err.to_string();
    assert!(
        text.starts_with("line 3 of the config file ")
            && text.contains("offers is refused: \"gpu RTX_4090\" (not a flag"),
        "{text}"
    );
}

#[test]
fn a_file_of_filters_cannot_turn_the_run_into_a_loop() {
    for (flag, line) in [("watch", "--watch 60"), ("changes", "--changes")] {
        let dir = std::env::temp_dir().join(format!("vas-config-{flag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("offers");
        std::fs::write(
            &file,
            format!(
                "--gpu RTX_5090
{line}
"
            ),
        )
        .unwrap();
        let err = read_flag_file(&file).unwrap_err();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(
            matches!(err, SkoposError::ConfigLine { line: 2, .. }),
            "{err}"
        );
        let text = err.to_string();
        assert!(
            text.contains(&format!("`--{flag}` cannot be set from a config file"))
                && text.contains("must not turn a run into a loop"),
            "{text}"
        );
    }
}

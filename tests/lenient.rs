//! What the readers do with text and numbers they do not trust: characters that hide or reorder what
//! is shown, and numbers written in the other way a serialiser may write them.

#![allow(clippy::unwrap_used, clippy::unnecessary_wraps)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion; and `outcome` returns the
// `ProbeOutcome` (an `Option`) that the API takes.

use vast_skopos::compact::json_lines;
use vast_skopos::instance::Instance;
use vast_skopos::probe::{LogReading, ProbeOutcome, clean, parse_output, strip};
use vast_skopos::render::{TableOptions, table};

/// Characters that must never reach a terminal from a box: a right-to-left override, a zero-width
/// space, a soft hyphen, a deprecated format character, a variation selector, a tag character (which
/// can spell a hidden message), an interlinear annotation, and the byte-order mark.
const HIDDEN: [char; 8] = [
    '\u{202E}',
    '\u{200B}',
    '\u{00AD}',
    '\u{206A}',
    '\u{FE0F}',
    '\u{E0041}',
    '\u{FFF9}',
    '\u{FEFF}',
];

fn running(label: &str) -> Instance {
    Instance::from_json(&serde_json::json!({"id": 1, "actual_status": "running", "label": label}))
        .unwrap()
}

fn outcome(text: &str) -> ProbeOutcome {
    Some(Ok(parse_output(text)))
}

#[test]
fn a_log_line_and_a_file_name_lose_every_hidden_character_not_only_the_control_ones() {
    for hidden in HIDDEN {
        let line = format!("epoch {hidden}41 {hidden}done");
        assert_eq!(strip(&line), "epoch 41 done", "{hidden:?}");
        assert_eq!(clean(&line), "epoch 41 done", "{hidden:?}");
        let probe = parse_output(&format!(
            "0, 1, 1, 2\n---vas-log---\nvas-file:/r/a{hidden}.log\nhello {hidden}world\n"
        ));
        assert_eq!(
            probe.log,
            Some(LogReading::Line {
                file: "/r/a.log".to_owned(),
                line: "hello world".to_owned()
            }),
            "{hidden:?}"
        );
    }
    // Ordinary text, accents and CJK are not touched.
    assert_eq!(strip("é 日本語 ok"), "é 日本語 ok");
}

#[test]
fn what_reaches_the_table_and_the_json_from_a_probe_is_clean() {
    let output = "0, 1, 1, 2\n---vas-log---\nvas-file:/r/s\u{202E}0/train.log\ne\u{200B}poch 41\n";
    let text = table(
        &[running("box")],
        &[outcome(output)],
        None,
        TableOptions::new().ssh(true),
        0.0,
    );
    assert!(text.contains("s0/train.log: epoch 41"), "{text}");
    for hidden in HIDDEN {
        assert!(!text.contains(hidden), "{hidden:?} in {text}");
    }
    let json = json_lines(&[running("box")], &[outcome(output)], None, None, 0.0);
    for hidden in HIDDEN {
        assert!(!json.contains(hidden), "{hidden:?} in {json}");
    }
}

#[test]
fn the_api_text_loses_the_same_characters() {
    for hidden in HIDDEN {
        let label = format!("me{hidden}37");
        let text = table(&[running(&label)], &[], None, TableOptions::new(), 0.0);
        assert!(text.contains("me37"), "{hidden:?}: {text}");
        assert!(!text.contains(hidden), "{hidden:?}: {text}");
    }
}

#[test]
fn what_tells_machines_apart_is_read_leniently_on_an_offer_and_on_a_box() {
    use vast_skopos::offer::Offer;
    let row = serde_json::json!({"id": 1, "cpu_name": "AMD\u{202E} EPYC\u{1b}", "driver_version": " 580.142 ",
        "dlperf": "199.2", "pcie_bw": 26.9, "gpu_max_power": "575", "dph_total": 0.5});
    let o = Offer::from_json(&row).unwrap();
    assert_eq!(
        (o.cpu_name.as_deref(), o.driver_version.as_deref()),
        (Some("AMD EPYC"), Some("580.142"))
    );
    assert_eq!(
        (o.dlperf, o.pcie_bw, o.gpu_max_power),
        (Some(199.2), Some(26.9), Some(575.0))
    );
    assert_eq!(o.dlperf_per_dollar(), Some(398.4));
    let i = Instance::from_json(&row).unwrap();
    assert_eq!(
        (
            i.cpu_name.as_deref(),
            i.driver_version.as_deref(),
            i.dlperf,
            i.pcie_bw
        ),
        (Some("AMD EPYC"), Some("580.142"), Some(199.2), Some(26.9))
    );
    // Garbage costs a dash, never the row.
    let odd = serde_json::json!({"id": 2, "cpu_name": 7, "driver_version": [], "dlperf": "NaN",
        "pcie_bw": "fast", "gpu_max_power": {}, "dph_total": -1.0});
    let o = Offer::from_json(&odd).unwrap();
    assert_eq!(
        (
            o.cpu_name,
            o.driver_version,
            o.dlperf,
            o.pcie_bw,
            o.gpu_max_power
        ),
        (None, None, None, None, None)
    );
    let below_zero =
        Offer::from_json(&serde_json::json!({"id": 3, "dlperf": 199.2, "dph_total": -1.0}))
            .unwrap();
    assert_eq!(below_zero.dlperf_per_dollar(), None);
    let i = Instance::from_json(&odd).unwrap();
    assert_eq!((i.cpu_name, i.dlperf, i.pcie_bw), (None, None, None));
}

#[test]
fn the_direct_route_is_read_from_the_ports_map_leniently() {
    let with = |ports: serde_json::Value| {
        Instance::from_json(
            &serde_json::json!({"id": 1, "public_ipaddr": "79.117.26.24", "ports": ports}),
        )
        .unwrap()
        .direct_endpoint()
        .map(|(host, port)| (host.to_owned(), port))
    };
    let at = |port: u16| Some((String::from("79.117.26.24"), port));
    // As read on 2026-10-26: one entry per address family, the port as a string.
    let real = serde_json::json!({"22/tcp": [{"HostIp": "0.0.0.0", "HostPort": "20674"},
        {"HostIp": "::", "HostPort": "20674"}], "1111/tcp": [{"HostIp": "0.0.0.0", "HostPort": "20962"}]});
    assert_eq!(with(real), at(20_674));
    assert_eq!(
        with(serde_json::json!({"22/tcp": [{"HostPort": 2222}]})),
        at(2222)
    );
    // The first entry that holds a port wins; garbage costs the column, never the row.
    assert_eq!(
        with(
            serde_json::json!({"22/tcp": [{"HostPort": "x"}, {"HostPort": "0"}, {"HostPort": "22022"}]})
        ),
        at(22_022)
    );
    for odd in [
        serde_json::json!({}),
        serde_json::json!({"22/tcp": []}),
        serde_json::json!({"22/tcp": [{"HostPort": "70000"}]}),
        serde_json::json!({"22/tcp": "55814"}),
        serde_json::json!(null),
        serde_json::json!({"1111/tcp": [{"HostPort": "20962"}]}),
    ] {
        assert_eq!(with(odd.clone()), None, "{odd}");
    }
    let no_ip = Instance::from_json(
        &serde_json::json!({"id": 1, "ports": {"22/tcp": [{"HostPort": "55814"}]}}),
    )
    .unwrap();
    assert_eq!(
        (no_ip.direct_ssh_port, no_ip.direct_endpoint()),
        (Some(55_814), None)
    );
}

#[test]
fn a_row_that_says_port_zero_has_no_endpoint_to_probe() {
    let row = |port: u64| {
        Instance::from_json(&serde_json::json!(
            {"id": 1, "ssh_host": "ssh4.vast.ai", "ssh_port": port}))
        .unwrap()
    };
    assert_eq!(row(0).ssh_endpoint(), None);
    assert_eq!(row(22).ssh_endpoint(), Some(("ssh4.vast.ai", 22)));
}

#[test]
fn an_integer_written_as_a_float_or_a_float_string_is_still_an_integer() {
    let read = |row: serde_json::Value| Instance::from_json(&row);
    let i = read(
        serde_json::json!({"id": 123.0, "ssh_port": "23484.0", "num_gpus": 2.0,
        "machine_id": "100682.0", "host_id": 5}),
    )
    .unwrap();
    assert_eq!(
        (i.id, i.ssh_port, i.num_gpus, i.machine_id, i.host_id),
        (123, Some(23_484), Some(2), Some(100_682), Some(5))
    );
    // Not an integer: no row, or no value; never a rounded one.
    assert!(read(serde_json::json!({"id": 1.5})).is_none());
    assert!(read(serde_json::json!({"id": -1.0})).is_none());
    assert!(read(serde_json::json!({"id": 1e300})).is_none());
    assert!(read(serde_json::json!({"id": 9_007_199_254_740_993.0_f64})).is_none());
    assert_eq!(
        read(serde_json::json!({"id": "1e3"})).map(|i| i.id),
        Some(1000)
    );
    assert!(read(serde_json::json!({"id": "x"})).is_none());
    let odd =
        read(serde_json::json!({"id": 1, "num_gpus": 2.5, "ssh_port": "NaN", "host_id": null}))
            .unwrap();
    assert_eq!(
        (odd.num_gpus, odd.ssh_port, odd.host_id),
        (None, None, None)
    );
}

#[test]
fn a_line_break_in_the_apis_text_becomes_one_space_not_a_glue() {
    // 2026-10-28: a status message holding two lines read `Verifying Checksumc455daa85bb6:`.
    let row = serde_json::json!({"id": 58_073_021, "actual_status": "loading",
        "status_msg": "c455daa85bb6: Verifying Checksum\nc455daa85bb6:\r\n\r\n",
        "label": "me41\tbox 2", "geolocation": "Buenos Aires,\u{2028}AR"});
    let i = Instance::from_json(&row).unwrap();
    assert_eq!(
        i.status_msg.as_deref(),
        Some("c455daa85bb6: Verifying Checksum c455daa85bb6:")
    );
    assert_eq!(i.label.as_deref(), Some("me41 box 2"));
    assert_eq!(i.geolocation.as_deref(), Some("Buenos Aires, AR"));
    // A break beside a space adds no second space; a run of breaks is one space; the other
    // control characters are still dropped, not spaced.
    let odd = serde_json::json!({"id": 1, "label": "a \n b\r\n\tc\u{1b}d"});
    assert_eq!(
        Instance::from_json(&odd).unwrap().label.as_deref(),
        Some("a b cd")
    );
    let text = table(&[i], &[], None, TableOptions::new(), 0.0);
    assert!(
        text.contains("58073021 loading: c455daa85bb6: Verifying Checksum c455daa85bb6:\n"),
        "{text}"
    );
}

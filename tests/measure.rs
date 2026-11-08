//! The harness of the learning-curve measurement (`docs/roadmap-v0.5.0.md`, part 4): a mock of
//! vast.ai that keeps running, four boxes and a page of offers, and a fake `ssh` and `nvidia-smi`
//! that answer as those boxes. It tests nothing: it is one `#[ignore]` test that serves until it
//! is killed, so a fresh agent can be given `vas` and real tasks without a key, a rental or the
//! network. Linux only, as `tests/until_cli.rs` (the remote command needs `timeout`; WSL will do):
//!
//! ```sh
//! cargo test --test measure -- --ignored --nocapture
//! ```
//!
//! It prints the base URL and writes `$TMPDIR/vas-measure/env.sh` (`VAST_URL`, a dummy
//! `VAST_API_KEY`, an empty `HOME` and `XDG_CONFIG_HOME`, so no config file of the measurer's is
//! read, and the fake `ssh` and this build's `vas` first on the `PATH`);
//! `$TMPDIR/vas-measure/shell 'vas --ssh'` runs one command under it. Nothing reaches vast.ai or a
//! real box: the boxes' hosts end in `.invalid` and their direct address is a documentation one
//! (TEST-NET-3), so even a real `ssh` given them goes nowhere. Restart it before each run: box 2's
//! first loss is written 40 s after the first search (`--grep` or `--until`) for it, once. Its
//! temporary folder's path must hold no blank, `#`, `&` or `\` (the scripts and `sed` take it as
//! is).
//!
//! The boxes, each one task's answer:
//!
//! - `56533728` (port 24016): at work, its limit at 400 W under a 575 W default, held by the power
//!   cap (`400/575W+power_cap`); its log already holds `initial loss`;
//! - `56558627` (port 20294): at work on a board whose maximum is 450 W (`450W<575`); its log gets
//!   `initial loss` late (a wait that does not name the box ends on `56533728`'s line instead);
//! - `57787711` (port 31002): an idle card whose counter reads 100 % (`100%?`), no log;
//! - `57340012`: `loading`, vast.ai's message naming the image's layers arriving.
//!
//! The offers: RTX 5090s and one RTX 4090, against `56533728`'s $0.491/h. The cheaper verified
//! RTX 5090s are `52010001` (the same CPU, host and `DLPerf`: the closest) and `52010003`; `52010002`
//! is cheapest but unverified, `52010006` deverified, and `52010004` is `56533728`'s own machine.

#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
// EXPLICIT: harness code -- a failed unwrap IS a harness that cannot start, said at once.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::{serve, unix_now};

/// The fake `ssh`: the box is chosen by `-p PORT` (a folder under `boxes/`; none is a refused
/// connection), and the remote command (the last argument) runs with `sh`, `/root/` meaning the
/// box's own `root/` (and its folder taken out of what it prints, so paths read as on a box).
/// Box 2's late line is appended 40 s after the first search (`grep -a -E`) asked for it.
const FAKE_SSH: &str = r#"#!/bin/sh
dir="__DIR__"
port=
prev=
for a; do
    [ "$prev" = "-p" ] && port=$a
    prev=$a
done
for last; do :; done
box="$dir/boxes/$port"
if [ -z "$port" ] || [ ! -d "$box" ]; then
    echo "ssh: connect to host box port $port: Connection refused" >&2
    exit 255
fi
if [ -f "$box/late" ]; then
    case "$last" in
        *"grep -a -E"*) [ -f "$box/clock" ] || date +%s > "$box/clock" ;;
    esac
    if [ -f "$box/clock" ] && [ ! -f "$box/written" ] &&
        [ "$(date +%s)" -ge $(( $(cat "$box/clock") + 40 )) ]; then
        cat "$box/late" >> "$box/root/runs/s0/train.log"
        touch "$box/written"
    fi
fi
cmd=$(printf '%s' "$last" | sed "s#/root/#$box/root/#g")
VAS_MEASURE_BOX="$box" sh -c "$cmd" | sed "s#$box##g"
"#;

/// The fake `nvidia-smi`: each query's answer is a file of the box the fake `ssh` named.
const FAKE_SMI: &str = r#"#!/bin/sh
case "$*" in
    *utilization.gpu*) f=util ;;
    *power.max_limit*) f=max ;;
    *power.draw*) f=power ;;
    *clocks_event_reasons*|*clocks_throttle_reasons*) f=events ;;
    *pci.bus_id*) f=bus ;;
    *query-compute-apps*) f=apps ;;
    *) exit 2 ;;
esac
cat "$VAS_MEASURE_BOX/smi/$f"
"#;

/// Runs one command line with the harness's environment: `shell 'vas --ssh'`.
const SHELL: &str = r#"#!/bin/sh
. "__DIR__/env.sh"
exec bash -c "$*"
"#;

/// One box the fake `ssh` answers as: its port, what each `nvidia-smi` query prints, and its log.
struct FakeBox {
    /// The proxy port, which names the box to the fake `ssh`.
    port: u16,
    /// `nvidia-smi`'s answers: utilisation and memory, power and clocks, clock event reasons,
    /// bus id, compute processes, the board's maximum power.
    smi: [&'static str; 6],
    /// The log's lines now, when the box has one (`root/runs/s0/train.log`).
    log: Option<&'static str>,
    /// A line appended to the log 40 s after the first search for it.
    late: Option<&'static str>,
}

/// The three running boxes.
const BOXES: [FakeBox; 3] = [
    FakeBox {
        port: 24016,
        smi: [
            "0, 99, 24809, 32607\n",
            "0, 399.86, 400.00, 575.00, 1717, 3090\n",
            "0, 0x0000000000000004\n",
            "0, 00000000:C1:00.0\n",
            "00000000:C1:00.0, 24390\n",
            "0, 600.00\n",
        ],
        log: Some(
            "loading data\nstep 0 initial loss 2.4512\nstep 100 loss 2.0117\nstep 200 loss 1.8843\n",
        ),
        late: None,
    },
    FakeBox {
        port: 20294,
        smi: [
            "0, 98, 20480, 32607\n",
            "0, 441.20, 450.00, 450.00, 2610, 3090\n",
            "0, 0x0000000000000000\n",
            "0, 00000000:81:00.0\n",
            "00000000:81:00.0, 20060\n",
            "0, 450.00\n",
        ],
        log: Some("loading data\ncompiling the model\n"),
        late: Some("step 0 initial loss 2.3105\n"),
    },
    FakeBox {
        port: 31002,
        smi: [
            "0, 100, 2, 32607\n",
            "0, 31.40, 575.00, 575.00, 210, 3090\n",
            "0, 0x0000000000000001\n",
            "0, 00000000:41:00.0\n",
            "",
            "0, 600.00\n",
        ],
        log: None,
        late: None,
    },
];

/// The instances list: the four boxes, their ages counted from now.
fn instances() -> String {
    let now = unix_now();
    let row = |id: u64,
               label: &str,
               status: &str,
               message: Option<&str>,
               age_min: f64,
               port: u16,
               machine: u64,
               host: u64,
               dph: f64,
               place: &str| {
        serde_json::json!({
            "id": id, "label": label, "actual_status": status, "intended_status": "running",
            "status_msg": message, "gpu_name": "RTX 5090", "num_gpus": 1, "dph_total": dph,
            "start_date": age_min.mul_add(-60.0, now), "ssh_host": format!("ssh{}.vast.invalid", port % 7),
            "ssh_port": port, "image_runtype": "ssh", "public_ipaddr": "203.0.113.38",
            "ports": {"22/tcp": [{"HostIp": "0.0.0.0", "HostPort": (port + 31000).to_string()}]},
            "machine_id": machine, "host_id": host, "cpu_name": "AMD EPYC 7K62 48-Core Processor",
            "driver_version": "580.142", "dlperf": 199.2, "pcie_bw": 26.9, "geolocation": place,
        })
    };
    serde_json::json!({
        "instances": [
            row(56_533_728, "train a", "running", Some("success, running pytorch/pytorch"), 266.0,
                24016, 100_682, 406_325, 0.491, "United States, US"),
            row(56_558_627, "train b", "running", None, 51.0, 20294, 142_875, 88_310, 0.704,
                "Texas, US"),
            row(57_787_711, "spare", "running", None, 122.0, 31002, 133_570, 270_760, 0.512,
                "Estonia, EE"),
            row(57_340_012, "train c", "loading", Some("46b4edaa8d47: Pull complete"), 3.0,
                15550, 145_990, 99_812, 0.533, "Denmark, DK"),
        ],
        "next_token": null,
    })
    .to_string()
}

/// The offers page: every row the search may answer (`vas` re-checks each against the flags).
fn offers() -> String {
    let row = |id: u64,
               gpu: &str,
               dph: f64,
               verification: &str,
               machine: u64,
               host: u64,
               cpu: &str,
               dlperf: f64,
               place: &str| {
        serde_json::json!({
            "id": id, "gpu_name": gpu, "num_gpus": 1, "gpu_ram": 32607, "dph_total": dph,
            "cuda_max_good": 13.0, "disk_space": 800.0, "reliability2": 0.993, "inet_up": 900.0,
            "inet_down": 1800.0, "duration": 6_442_672.9, "machine_id": machine, "host_id": host,
            "verification": verification, "verified": verification == "verified",
            "geolocation": place, "cpu_name": cpu, "driver_version": "580.142", "dlperf": dlperf,
            "pcie_bw": 26.1, "gpu_max_power": 575.0, "rentable": true, "rented": false,
        })
    };
    let epyc = "AMD EPYC 7K62 48-Core Processor";
    serde_json::json!({ "offers": [
        row(52_010_001, "RTX 5090", 0.452, "verified", 100_690, 406_325, epyc, 201.3, "United States, US"),
        row(52_010_002, "RTX 5090", 0.380, "unverified", 133_001, 288_001, "Intel Xeon Gold 6338", 160.4, "Vietnam, VN"),
        row(52_010_003, "RTX 5090", 0.474, "verified", 145_991, 99_812, "AMD EPYC 7B13 64-Core Processor", 196.0, "Denmark, DK"),
        row(52_010_004, "RTX 5090", 0.612, "verified", 100_682, 406_325, epyc, 199.2, "United States, US"),
        row(52_010_005, "RTX 4090", 0.310, "verified", 120_011, 51_200, epyc, 120.5, "Canada, CA"),
        row(52_010_006, "RTX 5090", 0.410, "deverified", 150_220, 61_004, epyc, 198.7, "Poland, PL"),
    ]})
    .to_string()
}

/// The mock's answers: the instances, the account, the offers.
fn respond(target: &str) -> (u16, String) {
    if target.starts_with("/api/v1/instances/") {
        (200, instances())
    } else if target.starts_with("/api/v0/users/current/") {
        (200, String::from(r#"{"credit": 23.47, "balance": 0}"#))
    } else if target.starts_with("/api/v0/bundles/") {
        (200, offers())
    } else {
        (404, String::from(r#"{"error": "not found"}"#))
    }
}

/// Writes `text` to `path`, executable.
fn script(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
#[ignore = "the measurement's standing mock: it serves until killed (see the module docs)"]
fn serve_the_measurement() {
    let dir = std::env::temp_dir().join("vas-measure");
    let _ = fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(dir.join("config")).unwrap();
    fs::create_dir_all(dir.join("home")).unwrap();
    let at = dir.display().to_string();
    script(&bin.join("ssh"), &FAKE_SSH.replace("__DIR__", &at));
    script(&bin.join("nvidia-smi"), FAKE_SMI);
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_vas"), bin.join("vas")).unwrap();
    for b in &BOXES {
        let home = dir.join("boxes").join(b.port.to_string());
        fs::create_dir_all(home.join("smi")).unwrap();
        for (name, text) in ["util", "power", "events", "bus", "apps", "max"]
            .iter()
            .zip(b.smi)
        {
            fs::write(home.join("smi").join(name), text).unwrap();
        }
        if let Some(log) = b.log {
            fs::create_dir_all(home.join("root/runs/s0")).unwrap();
            fs::write(home.join("root/runs/s0/train.log"), log).unwrap();
        }
        if let Some(late) = b.late {
            fs::write(home.join("late"), late).unwrap();
        }
    }
    let (base, seen) = serve(respond);
    fs::write(
        dir.join("env.sh"),
        format!(
            "export VAST_URL={base}\nexport VAST_API_KEY=measure-dummy-key\n\
             export HOME='{at}/home'\nexport XDG_CONFIG_HOME='{at}/config'\n\
             export PATH='{}':\"$PATH\"\n",
            bin.display()
        ),
    )
    .unwrap();
    script(&dir.join("shell"), &SHELL.replace("__DIR__", &at));
    println!("vas-measure: serving at {base}; run commands with {at}/shell 'vas --ssh'");
    let mut said = 0;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
        let requests = seen.lock().unwrap();
        for request in requests.iter().skip(said) {
            println!("vas-measure: {} {}", request.method, request.target);
        }
        said = requests.len();
    }
}

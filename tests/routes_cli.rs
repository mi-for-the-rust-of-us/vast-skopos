//! `vas --ssh` over two routes, end to end: a fake `ssh` first on the `PATH` refuses the proxy's port as
//! OpenSSH does (`Connection refused`, exit 255) and, on the box's direct port, runs the remote command
//! with the local `sh` (and a fake `nvidia-smi`). Every attempt is logged, so the test counts them.
//! Linux only, as `tests/until_cli.rs`.

#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
// EXPLICIT: test code -- a failed unwrap or index IS the failed assertion.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use common::{RemoveOnDrop, serve};

/// The proxy's port (always refused here) and the box's direct port (refused or answered, as the
/// scratch directory says).
const PROXY_PORT: u16 = 22;
const DIRECT_PORT: u16 = 2222;

/// One running box with a proxy endpoint and a direct one; and the account.
fn one_box(target: &str) -> (u16, String) {
    if target.starts_with("/api/v1/instances/") {
        (
            200,
            r#"{"instances": [{"id": 77, "actual_status": "running", "intended_status": "running",
                "dph_total": 0.5, "start_date": 1.0, "ssh_host": "box.invalid", "ssh_port": 22,
                "public_ipaddr": "192.0.2.7",
                "ports": {"22/tcp": [{"HostIp": "0.0.0.0", "HostPort": "2222"}]}}],
                "next_token": null}"#
                .to_owned(),
        )
    } else {
        (200, r#"{"credit": 5.0, "balance": 0}"#.to_owned())
    }
}

/// A scratch directory with a `bin` holding the fake `ssh` and `nvidia-smi`. The fake `ssh` logs
/// `host:port` to `attempts`, refuses the proxy port, and on the direct port refuses too when a
/// file `closed` exists, else runs its last argument with `sh`.
fn scratch(name: &str, direct_open: bool) -> RemoveOnDrop {
    let dir = std::env::temp_dir().join(format!("vas-routes-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let script = |file: &str, text: &str| {
        let path = bin.join(file);
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    };
    let root = dir.display();
    script(
        "ssh",
        &format!(
            r#"#!/bin/sh
port=22; target=
while [ $# -gt 1 ]; do
  case "$1" in -p) port="$2"; shift ;; root@*) target="${{1#root@}}" ;; esac
  shift
done
echo "$target:$port" >> '{root}/attempts'
if [ "$port" = {PROXY_PORT} ] || [ "$port" != {DIRECT_PORT} ] || [ -e '{root}/closed' ]; then
  echo "ssh: connect to host $target port $port: Connection refused" >&2
  exit 255
fi
exec sh -c "$1"
"#
        ),
    );
    script(
        "nvidia-smi",
        "#!/bin/sh\ncase \"$*\" in *utilization.gpu*) echo \"0, 99, 1000, 32607\" ;; esac\n",
    );
    if !direct_open {
        fs::write(dir.join("closed"), "").unwrap();
    }
    RemoveOnDrop(dir)
}

/// `vas args`, the fake `ssh` first on the `PATH`, against the mock at `base`: its stdout and the
/// attempts the fake `ssh` logged.
fn vas(base: &str, dir: &Path, args: &[&str]) -> (String, Vec<String>) {
    let path = format!(
        "{}:{}",
        dir.join("bin").display(),
        std::env::var("PATH").unwrap()
    );
    let out = Command::new(env!("CARGO_BIN_EXE_vas"))
        .args(args)
        .env("VAST_URL", base)
        .env("VAST_API_KEY", "k3y")
        .env("HOME", dir)
        .env("PATH", path)
        .output()
        .unwrap();
    let attempts = fs::read_to_string(dir.join("attempts")).unwrap_or_default();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        attempts.lines().map(str::to_owned).collect(),
    )
}

#[test]
fn a_refused_proxy_is_followed_by_one_attempt_at_the_direct_address_whose_readings_fill_the_row() {
    let dir = scratch("open", true);
    let (base, _) = serve(one_box);
    let (stdout, attempts) = vas(&base, &dir.0, &["--ssh", "--wide"]);
    assert_eq!(
        attempts,
        ["box.invalid:22", "192.0.2.7:2222"],
        "the proxy, then the direct address, once each"
    );
    let row = stdout.lines().find(|l| l.starts_with("77")).unwrap();
    assert!(row.contains("192.0.2.7:2222"), "DIRECT: {row}");
    assert!(
        row.contains("99%"),
        "the readings came by the direct route: {row}"
    );
    assert!(row.ends_with("(direct) -"), "and say so: {row}");
    let (json, _) = vas(&base, &dir.0, &["--ssh", "--compact"]);
    let first: serde_json::Value = serde_json::from_str(json.lines().next().unwrap()).unwrap();
    assert_eq!(first["probe"]["route"], "direct", "{first}");
}

#[test]
fn a_box_neither_route_reached_says_both_reasons_after_two_attempts() {
    let dir = scratch("closed", false);
    let (base, _) = serve(one_box);
    let (stdout, attempts) = vas(&base, &dir.0, &["--ssh"]);
    assert_eq!(attempts.len(), 2, "never a third attempt: {attempts:?}");
    let row = stdout.lines().find(|l| l.starts_with("77")).unwrap();
    assert!(row.contains("UNREACHABLE "), "{row}");
    assert!(
        row.contains("port 22: Connection refused (direct 192.0.2.7:2222: ssh: connect to host 192.0.2.7 port 2222: Connection refused)"),
        "{row}"
    );
    let (json, _) = vas(&base, &dir.0, &["--ssh", "--compact"]);
    let first: serde_json::Value = serde_json::from_str(json.lines().next().unwrap()).unwrap();
    assert_eq!(first["probe"]["error_kind"], "unreachable", "{first}");
    assert!(
        first["probe"]["direct_error"]
            .as_str()
            .is_some_and(|e| e.starts_with("192.0.2.7:2222: ")),
        "{first}"
    );
}

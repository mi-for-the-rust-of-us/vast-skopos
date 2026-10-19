//! The local mock of the vast.ai API shared by the test crates, and its helpers. No network.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion; and each test crate that includes
// this module uses only part of it, so the rest reads as dead there.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// Unix seconds now.
pub fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

/// One request as the mock saw it.
#[derive(Debug, Clone)]
pub struct Request {
    /// The method (`GET`, ...).
    pub method: String,
    /// The request target: path and query.
    pub target: String,
    /// The `Authorization` header, when one was sent.
    pub authorization: Option<String>,
    /// When the mock read it.
    pub at: std::time::Instant,
}

/// Every request the mock saw, in order.
pub type Seen = Arc<Mutex<Vec<Request>>>;

/// The mock's answer to a request target: status and body.
pub type Respond = fn(&str) -> (u16, String);

/// The mock's answer with headers: status, extra headers (name, value), body.
pub type RespondWith = fn(&str) -> (u16, Vec<(String, String)>, String);

/// Serves `respond(target) -> (status, body)` on a free port, one request per connection; returns
/// the base URL and the requests seen.
pub fn serve(respond: Respond) -> (String, Seen) {
    serve_inner(move |target| {
        let (status, body) = respond(target);
        (status, Vec::new(), body)
    })
}

/// [`serve`], with headers on the answer (a rate limit's, say).
pub fn serve_with(respond: RespondWith) -> (String, Seen) {
    serve_inner(respond)
}

/// The mock itself: `respond` answers each request.
fn serve_inner<F>(respond: F) -> (String, Seen)
where
    F: Fn(&str) -> (u16, Vec<(String, String)>, String) + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let mut words = first.split_whitespace();
            let method = words.next().unwrap_or("").to_owned();
            let target = words.next().unwrap_or("").to_owned();
            let mut authorization = None;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("authorization")
                {
                    authorization = Some(value.trim().to_owned());
                }
            }
            log.lock().unwrap().push(Request {
                method,
                target: target.clone(),
                authorization,
                at: std::time::Instant::now(),
            });
            let (status, headers, body) = respond(&target);
            let mut extra = String::new();
            for (name, value) in &headers {
                let _ = write!(extra, "{name}: {value}\r\n");
            }
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    (base, seen)
}

/// `%XX` and `+` decoded (a query value as a client may encode it).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap()
}

/// The `q` parameter of a request target, decoded and parsed as JSON.
pub fn decode_q(target: &str) -> serde_json::Value {
    let (_, query) = target.split_once('?').expect("a query");
    let q = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("q="))
        .expect("a q parameter");
    serde_json::from_str(&percent_decode(q)).unwrap()
}

/// A fixture file of `tests/fixtures/`, as text.
pub fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

/// How long a test waits for a process before it fails: one deadline for the whole wait.
pub const PROCESS_DEADLINE: std::time::Duration = std::time::Duration::from_secs(20);

/// The stdout lines of `child` (spawned with a piped stdout) until one containing `marker` has been
/// seen `times` times; then it is killed. ONE deadline covers the whole read, not one per line: a
/// `vas` that never reaches the marker but keeps printing must fail the test, not feed it forever.
pub fn read_lines_until(
    child: &mut std::process::Child,
    marker: &str,
    times: usize,
) -> Vec<String> {
    let stdout = child.stdout.take().expect("a piped stdout");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufRead::lines(BufReader::new(stdout)).map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = std::time::Instant::now() + PROCESS_DEADLINE;
    let (mut lines, mut seen) = (Vec::new(), 0);
    while seen < times {
        let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) else {
            break;
        };
        match rx.recv_timeout(left) {
            Ok(line) => {
                seen += usize::from(line.contains(marker));
                lines.push(line);
            }
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(seen, times, "{marker:?} seen {seen} times in {lines:#?}");
    lines
}

/// The exit status of `child`, which must exit within [`PROCESS_DEADLINE`]: a watch that should
/// have given up and loops on fails the test (and is killed), it does not hang it.
pub fn exit_within_deadline(child: &mut std::process::Child) -> std::process::ExitStatus {
    let deadline = std::time::Instant::now() + PROCESS_DEADLINE;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("still running after {PROCESS_DEADLINE:?}: it should have exited");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Removes the directory when dropped: a home a test made does not outlive it, even when an
/// assertion panics first.
pub struct RemoveOnDrop(pub std::path::PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

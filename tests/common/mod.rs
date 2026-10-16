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

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// One request as the mock saw it.
#[derive(Debug, Clone)]
pub struct Request {
    /// The method (`GET`, ...).
    pub method: String,
    /// The request target: path and query.
    pub target: String,
    /// The `Authorization` header, when one was sent.
    pub authorization: Option<String>,
}

/// Every request the mock saw, in order.
pub type Seen = Arc<Mutex<Vec<Request>>>;

/// The mock's answer to a request target: status and body.
pub type Respond = fn(&str) -> (u16, String);

/// Serves `respond(target) -> (status, body)` on a free port, one request per connection; returns
/// the base URL and the requests seen.
pub fn serve(respond: Respond) -> (String, Seen) {
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
            });
            let (status, body) = respond(&target);
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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

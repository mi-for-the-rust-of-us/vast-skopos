//! The client against a local mock of the two endpoints: pagination, the auth header, the
//! account, and the error statuses. No network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use vast_skopos::SkoposError;
use vast_skopos::api::Client;
use vast_skopos::key::ApiKey;

/// One request as the mock saw it: the request target and the `Authorization` header.
type Seen = Arc<Mutex<Vec<(String, String)>>>;

/// Serves `respond(target) -> (status, body)` on a free port, one request per connection.
fn serve(respond: fn(&str) -> (u16, String)) -> (String, Seen) {
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
            let target = first.split_whitespace().nth(1).unwrap_or("").to_owned();
            let mut auth = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line
                    .strip_prefix("authorization: ")
                    .or_else(|| line.strip_prefix("Authorization: "))
                {
                    v.trim().clone_into(&mut auth);
                }
            }
            log.lock().unwrap().push((target.clone(), auth));
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

/// Two pages of instances, then the account.
fn vast(target: &str) -> (u16, String) {
    if target.starts_with("/api/v1/instances/") {
        if target.contains("after_token=") {
            (200, r#"{"instances": [{"id": 3, "actual_status": "exited", "dph_total": 0.1}], "next_token": null}"#.to_owned())
        } else {
            (200, r#"{"instances": [
                {"id": 1, "actual_status": "running", "dph_total": 0.643, "gpu_name": "RTX 5090",
                 "num_gpus": 1, "ssh_host": "ssh4.vast.ai", "ssh_port": 23484, "start_date": 1.0},
                {"id": 2, "actual_status": "loading", "intended_status": "running", "dph_total": 0.608},
                {"label": "a row without an id is skipped"}
              ], "next_token": "tok-2"}"#.to_owned())
        }
    } else if target.starts_with("/api/v0/users/current") {
        (
            200,
            r#"{"credit": 5.94, "balance": 0.0, "api_key": "NEVER-READ"}"#.to_owned(),
        )
    } else {
        (404, "{}".to_owned())
    }
}

#[test]
fn pages_are_followed_and_the_key_is_sent_as_a_bearer() {
    let (base, seen) = serve(vast);
    let client = Client::with_base(ApiKey::new("k3y").unwrap(), &base);
    let rows = client.instances().unwrap();
    assert_eq!(rows.iter().map(|i| i.id).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(rows[0].ssh_endpoint(), Some(("ssh4.vast.ai", 23484)));
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "two pages");
    assert!(seen[0].0.contains("limit=25") && seen[0].0.contains("order_by="));
    assert!(seen[1].0.contains("after_token=tok-2"));
    assert!(seen.iter().all(|(_, auth)| auth == "Bearer k3y"));
}

#[test]
fn the_account_reads_credit_and_never_the_key() {
    let (base, _) = serve(vast);
    let client = Client::with_base(ApiKey::new("k3y").unwrap(), &base);
    let account = client.account().unwrap();
    assert_eq!((account.credit, account.balance), (Some(5.94), Some(0.0)));
    assert!(!format!("{account:?}").contains("NEVER-READ"));
}

#[test]
fn a_refused_key_is_a_status_error_with_a_hint_and_no_key() {
    let (base, _) = serve(|_| (401, r#"{"error": "unauthorized"}"#.to_owned()));
    let client = Client::with_base(ApiKey::new("s3cret").unwrap(), &base);
    let err = client.instances().unwrap_err();
    assert!(matches!(err, SkoposError::Status { status: 401, .. }));
    let text = err.to_string();
    assert!(
        text.contains("API key") && !text.contains("s3cret"),
        "{text}"
    );
}

#[test]
fn a_page_without_instances_is_a_json_error() {
    let (base, _) = serve(|_| (200, r#"{"rows": []}"#.to_owned()));
    let client = Client::with_base(ApiKey::new("k").unwrap(), &base);
    assert!(matches!(
        client.instances().unwrap_err(),
        SkoposError::Json { .. }
    ));
}

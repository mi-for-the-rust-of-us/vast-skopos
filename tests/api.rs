//! The clients against a local mock of the endpoints: pagination, the auth header, the account,
//! the keyless offer search, and the error statuses. No network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
// EXPLICIT: test code -- a failed unwrap IS the failed assertion.

mod common;

use common::{Respond, decode_q, fixture, serve};
use vast_skopos::SkoposError;
use vast_skopos::api::{Client, SearchClient};
use vast_skopos::key::ApiKey;
use vast_skopos::offer::{OFFER_CAP, OfferQuery, OfferSort, select};

/// Two pages of instances, then the account, then the offer search (the fixture's real rows).
fn vast(target: &str) -> (u16, String) {
    if target.starts_with("/api/v1/instances/") {
        if target.contains("after_token=") {
            (200, r#"{"instances": [{"id": 3, "actual_status": "exited", "dph_total": 0.1}], "next_token": null}"#.to_owned())
        } else {
            (200, r#"{"instances": [
                {"id": 1, "actual_status": "running", "dph_total": 0.643, "gpu_name": "RTX 5090",
                 "num_gpus": 1, "ssh_host": "ssh4.vast.ai", "ssh_port": 23484, "start_date": 1.0,
                 "machine_id": 100682, "host_id": "406325"},
                {"id": 2, "actual_status": "loading", "intended_status": "running", "dph_total": 0.608},
                {"label": "a row without an id is skipped"}
              ], "next_token": "tok-2"}"#.to_owned())
        }
    } else if target.starts_with("/api/v0/users/current") {
        (
            200,
            r#"{"credit": 5.94, "balance": 0.0, "api_key": "NEVER-READ"}"#.to_owned(),
        )
    } else if target.starts_with("/api/v0/bundles/?q=") {
        (200, fixture("offers.json"))
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
    assert_eq!(
        (rows[0].machine_id, rows[0].host_id),
        (Some(100_682), Some(406_325)),
        "the two ids, a number and a numeric string, each in its own field"
    );
    assert_eq!((rows[1].machine_id, rows[1].host_id), (None, None));
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "two pages");
    assert!(seen[0].target.contains("limit=25") && seen[0].target.contains("order_by="));
    assert!(seen[1].target.contains("after_token=tok-2"));
    assert!(
        seen.iter()
            .all(|r| r.method == "GET" && r.authorization.as_deref() == Some("Bearer k3y"))
    );
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

#[test]
fn a_repeated_next_token_is_an_error_not_a_loop() {
    let (base, seen) = serve(|_| {
        (
            200,
            r#"{"instances": [{"id": 1}], "next_token": "same"}"#.to_owned(),
        )
    });
    let client = Client::with_base(ApiKey::new("k").unwrap(), &base);
    assert!(matches!(
        client.instances().unwrap_err(),
        SkoposError::Json { .. }
    ));
    assert_eq!(seen.lock().unwrap().len(), 2, "stopped at the first repeat");
}

#[test]
fn neither_the_client_nor_a_connection_error_shows_the_key() {
    // Nothing listens on port 9 here: the request fails before any answer.
    let client = Client::with_base(ApiKey::new("s3cret").unwrap(), "http://127.0.0.1:9");
    assert!(!format!("{client:?}").contains("s3cret"));
    let err = client.account().unwrap_err();
    assert!(matches!(err, SkoposError::Http { .. }));
    assert!(!err.to_string().contains("s3cret"), "{err}");
}

#[test]
fn a_redirect_is_reported_not_followed() {
    let (base, seen) = serve(|_| (302, "{}".to_owned()));
    let client = Client::with_base(ApiKey::new("k").unwrap(), &base);
    let err = client.account().unwrap_err();
    assert!(matches!(err, SkoposError::Status { status: 302, .. }));
    assert!(
        err.to_string().contains("a redirect: vas follows none"),
        "{err}"
    );
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn a_key_file_with_a_bom_or_in_utf16_reads_the_same_key() {
    let dir = std::env::temp_dir().join(format!("vas-key-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bom = dir.join("bom");
    std::fs::write(&bom, "\u{feff}abc123\r\n").unwrap();
    let utf16 = dir.join("utf16");
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend("abc123\r\n".encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&utf16, bytes).unwrap();
    let (base, seen) = serve(vast);
    for file in [&bom, &utf16] {
        let client = Client::with_base(ApiKey::from_file(file).unwrap(), &base);
        client.account().unwrap();
    }
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .all(|r| r.authorization.as_deref() == Some("Bearer abc123"))
    );
}

/// The askesis renting filters, as `vas offers` flags would set them.
fn askesis_query() -> OfferQuery {
    let mut q = OfferQuery::default();
    q.gpu = Some("RTX 5090".to_owned());
    q.num_gpus = Some(1);
    q.disk = Some(50);
    q.cuda = Some(12.8);
    q.reliability = Some(0.99);
    q.inet_up = Some(200.0);
    q.exclude_countries = vec!["CN".to_owned()];
    q.exclude_hosts = vec![362_355];
    q.exclude_machines = vec![142_875];
    q
}

#[test]
fn the_offer_search_is_one_keyless_get_carrying_every_constraint() {
    let (base, seen) = serve(vast);
    let page = SearchClient::with_base(&base)
        .offers(&askesis_query(), OfferSort::Price)
        .unwrap();
    assert_eq!(
        (
            page.offers.len(),
            page.returned,
            page.skipped,
            page.truncated
        ),
        (5, 5, 0, Some(false))
    );
    assert!(!page.maybe_cut());
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "GET");
    assert!(
        seen[0].target.starts_with("/api/v0/bundles/?q="),
        "{}",
        seen[0].target
    );
    assert_eq!(seen[0].authorization, None, "the search sends no key");
    assert_eq!(
        decode_q(&seen[0].target),
        serde_json::json!({
            "rentable": {"eq": true}, "rented": {"eq": false}, "type": "on-demand",
            "limit": 200, "allocated_storage": 50, "order": [["dph_total", "asc"]],
            "gpu_name": {"eq": "RTX 5090"}, "num_gpus": {"eq": 1}, "disk_space": {"gte": 50},
            "cuda_max_good": {"gte": 12.8}, "reliability2": {"gte": 0.99},
            "inet_up": {"gte": 200.0}, "geolocation": {"notin": ["CN"]},
            "host_id": {"notin": [362_355]}, "machine_id": {"notin": [142_875]}
        })
    );
}

#[test]
fn the_real_rows_are_rechecked_and_sorted_here() {
    let (base, _) = serve(vast);
    let client = SearchClient::with_base(&base);
    let ids = |q: &OfferQuery, sort| -> Vec<u64> {
        let page = client.offers(q, sort).unwrap();
        select(page.offers, q, sort).iter().map(|o| o.id).collect()
    };
    let identity = OfferQuery::default();
    // Two rows tie on price: the lower id first.
    assert_eq!(
        ids(&identity, OfferSort::Price),
        vec![49_574_222, 53_564_837, 44_614_424, 51_179_006, 51_054_445]
    );
    assert_eq!(
        ids(&identity, OfferSort::Reliability),
        vec![49_574_222, 44_614_424, 53_564_837, 51_179_006, 51_054_445]
    );
    // The mock ignores every constraint, as a server that dropped one would: the re-check holds.
    assert_eq!(
        ids(&askesis_query(), OfferSort::Price),
        Vec::<u64>::new(),
        "no 5090 here has both 99 % and 200 Mb/s up"
    );
    let mut fast = askesis_query();
    fast.reliability = None;
    assert_eq!(ids(&fast, OfferSort::Up), vec![51_179_006, 51_054_445]);
    let mut verified = OfferQuery::default();
    verified.verified = true;
    assert_eq!(
        ids(&verified, OfferSort::Price),
        vec![44_614_424, 51_179_006, 51_054_445]
    );
}

#[test]
fn an_answer_at_the_cap_may_be_cut() {
    let (base, _) = serve(|_| {
        let rows: Vec<String> = (0..64).map(|id| format!(r#"{{"id": {id}}}"#)).collect();
        (
            200,
            format!(r#"{{"offers": [{}], "truncated": false}}"#, rows.join(",")),
        )
    });
    let page = SearchClient::with_base(&base)
        .offers(&OfferQuery::default(), OfferSort::Price)
        .unwrap();
    assert_eq!((page.returned, page.truncated), (OFFER_CAP, Some(false)));
    assert!(
        page.maybe_cut(),
        "64 rows: the server's cap, whatever it says"
    );
    let (base, _) = serve(|_| {
        (
            200,
            r#"{"offers": [{"id": 1}, {"no": "id"}], "truncated": true}"#.to_owned(),
        )
    });
    let page = SearchClient::with_base(&base)
        .offers(&OfferQuery::default(), OfferSort::Price)
        .unwrap();
    assert_eq!((page.offers.len(), page.skipped), (1, 1));
    assert!(page.maybe_cut(), "the server said so");
}

#[test]
fn a_search_answer_without_offers_is_a_json_error() {
    let (base, _) = serve(|_| (200, r#"{"rows": []}"#.to_owned()));
    let err = SearchClient::with_base(&base)
        .offers(&OfferQuery::default(), OfferSort::Price)
        .unwrap_err();
    assert!(matches!(err, SkoposError::Json { .. }));
    assert!(err.to_string().contains("no `offers` list"), "{err}");
}

#[test]
fn the_search_statuses_carry_their_own_hints_and_never_the_query() {
    let cases: [(Respond, u16, &str); 5] = [
        (|_| (401, "{}".to_owned()), 401, "vas offers sends none"),
        (|_| (404, "{}".to_owned()), 404, "legacy GET form"),
        (|_| (405, "{}".to_owned()), 405, "legacy GET form"),
        (|_| (429, "{}".to_owned()), 429, "rate-limited"),
        (
            |_| (302, "{}".to_owned()),
            302,
            "a redirect: vas follows none",
        ),
    ];
    for (respond, status, hint) in cases {
        let (base, seen) = serve(respond);
        let err = SearchClient::with_base(&base)
            .offers(&askesis_query(), OfferSort::Price)
            .unwrap_err();
        assert!(
            matches!(err, SkoposError::Status { status: s, .. } if s == status),
            "{err}"
        );
        let text = err.to_string();
        assert!(
            text.contains(hint) && text.contains("/api/v0/bundles/"),
            "{text}"
        );
        assert!(!text.contains("RTX") && !text.contains("q="), "{text}");
        assert_eq!(seen.lock().unwrap().len(), 1, "one request, no retry");
    }
}

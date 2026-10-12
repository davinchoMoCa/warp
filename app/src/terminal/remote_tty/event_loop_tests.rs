use super::*;

#[test]
fn session_url_defaults_to_local_endpoint() {
    assert_eq!(
        session_url(None, None, 24, 80),
        "ws://127.0.0.1:3030/create?num_rows=24&num_cols=80"
    );
}

#[test]
fn session_url_uses_configured_endpoint_and_token() {
    assert_eq!(
        session_url(Some("ws://xps:3030/create"), Some("a b&c"), 40, 120),
        "ws://xps:3030/create?num_rows=40&num_cols=120&token=a%20b%26c"
    );
}

#[test]
fn session_url_ignores_blank_values() {
    assert_eq!(
        session_url(Some("  "), Some(""), 1, 2),
        "ws://127.0.0.1:3030/create?num_rows=1&num_cols=2"
    );
}

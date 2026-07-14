//! Safe Browser URL-restoration and bounded-history regressions.

use super::policy::BrowserUrlError;
use super::restoration::{admit_restorable_url, append_history, RestorableUrl, HISTORY_CAP};

#[test]
fn admits_public_and_loopback_http_urls_without_changing_safe_values() {
    for raw in [
        "https://example.com/path?q=ordinary#section",
        "http://localhost:5173/preview?mode=mobile",
        "http://127.0.0.1:3000/",
    ] {
        let admitted = admit_restorable_url(raw).expect("safe HTTP(S) URL");
        assert_eq!(admitted.value, raw);
        assert!(!admitted.manual_reopen_required);
    }
}

#[test]
fn rejects_credentials_schemes_controls_and_oversize_values() {
    let cases = [
        (
            "https://user:pass@example.com",
            BrowserUrlError::CredentialsBlocked,
        ),
        ("file:///tmp/private", BrowserUrlError::SchemeBlocked),
        ("javascript:alert(1)", BrowserUrlError::SchemeBlocked),
        ("https://example.com/\0hidden", BrowserUrlError::InvalidUrl),
    ];
    for (raw, expected) in cases {
        assert_eq!(admit_restorable_url(raw).unwrap_err(), expected, "{raw:?}");
    }
    let oversized = format!("https://example.com/{}", "a".repeat(8 * 1024));
    assert_eq!(
        admit_restorable_url(&oversized).unwrap_err(),
        BrowserUrlError::InvalidUrl
    );
}

#[test]
fn strips_secret_query_fragment_or_path_without_returning_the_secret() {
    let secret = format!("sk-{}", "a".repeat(24));
    for raw in [
        format!("https://example.com/path?token={secret}#safe"),
        format!("https://example.com/path?q=safe#{secret}"),
        format!("https://example.com/private/{secret}?q=safe"),
    ] {
        let admitted = admit_restorable_url(&raw).expect("valid URL with unsafe persistence data");
        assert!(admitted.manual_reopen_required);
        assert!(!admitted.value.contains(&secret));
        assert!(!format!("{admitted:?}").contains(&secret));
        if raw.contains("/private/") {
            assert_eq!(admitted.value, "https://example.com/");
        } else {
            assert_eq!(admitted.value, "https://example.com/path");
        }
    }
}

#[test]
fn detects_encoded_secrets_and_never_persists_a_secret_hostname() {
    let secret = format!("sk-{}", "p".repeat(24));
    let encoded = secret
        .bytes()
        .map(|byte| format!("%{byte:02X}"))
        .collect::<String>();
    let encoded_url = format!("https://example.com/path?next={encoded}");
    let admitted = admit_restorable_url(&encoded_url).expect("encoded secret URL");
    assert_eq!(admitted.value, "https://example.com/path");
    assert!(admitted.manual_reopen_required);
    assert!(!admitted.value.contains(&encoded));

    let host_url = format!("https://{secret}.example.com/private");
    let admitted = admit_restorable_url(&host_url).expect("secret-shaped host URL");
    assert_eq!(admitted.value, "https://redacted.invalid/");
    assert!(admitted.manual_reopen_required);
    assert!(!format!("{admitted:?}").contains(&secret));
}

#[test]
fn append_removes_forward_history_and_bounds_the_oldest_rows() {
    let mut history = vec![
        RestorableUrl::safe("https://example.com/one"),
        RestorableUrl::safe("https://example.com/two"),
        RestorableUrl::safe("https://example.com/three"),
    ];
    let (next, current) = append_history(history, 1, RestorableUrl::safe("https://new.test/"));
    assert_eq!(current, 2);
    assert_eq!(
        next.iter()
            .map(|item| item.value.as_str())
            .collect::<Vec<_>>(),
        [
            "https://example.com/one",
            "https://example.com/two",
            "https://new.test/"
        ]
    );

    history = (0..HISTORY_CAP)
        .map(|index| RestorableUrl::safe(format!("https://example.com/{index}")))
        .collect();
    let (bounded, current) = append_history(
        history,
        HISTORY_CAP - 1,
        RestorableUrl::safe("https://example.com/newest"),
    );
    assert_eq!(bounded.len(), HISTORY_CAP);
    assert_eq!(current, HISTORY_CAP - 1);
    assert_eq!(bounded[0].value, "https://example.com/1");
    assert_eq!(bounded.last().unwrap().value, "https://example.com/newest");
}

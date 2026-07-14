//! Pure top-level navigation policy for attacker-controlled browser content.

use std::net::{Ipv4Addr, Ipv6Addr};

pub const BROWSER_URL_BYTE_CAP: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserNetworkTarget {
    Public,
    Loopback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserUrlError {
    InvalidUrl,
    SchemeBlocked,
    CredentialsBlocked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedBrowserUrl {
    pub url: tauri::Url,
    pub target: BrowserNetworkTarget,
}

pub fn validate_browser_url(raw: &str) -> Result<ValidatedBrowserUrl, BrowserUrlError> {
    if raw.is_empty() || raw.len() > BROWSER_URL_BYTE_CAP || raw.chars().any(char::is_control) {
        return Err(BrowserUrlError::InvalidUrl);
    }

    let url = tauri::Url::parse(raw).map_err(|_| BrowserUrlError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(BrowserUrlError::SchemeBlocked);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(BrowserUrlError::CredentialsBlocked);
    }

    let host = url.host_str().ok_or(BrowserUrlError::InvalidUrl)?;
    let target = if is_loopback_host(host) {
        BrowserNetworkTarget::Loopback
    } else {
        BrowserNetworkTarget::Public
    };

    Ok(ValidatedBrowserUrl { url, target })
}

fn is_loopback_host(host: &str) -> bool {
    let normalized = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
        .to_ascii_lowercase();

    normalized == "localhost"
        || normalized.ends_with(".localhost")
        || normalized
            .parse::<Ipv4Addr>()
            .is_ok_and(|address| address.octets()[0] == 127)
        || normalized
            .parse::<Ipv6Addr>()
            .is_ok_and(|address| address.is_loopback())
}

pub const fn allow_popup() -> bool {
    false
}

pub const fn allow_download() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{
        allow_download, allow_popup, validate_browser_url, BrowserNetworkTarget, BrowserUrlError,
        BROWSER_URL_BYTE_CAP,
    };

    #[test]
    fn accepts_public_http_and_https_urls() {
        for raw in ["https://example.com/path", "http://example.com"] {
            let validated = validate_browser_url(raw).expect("public HTTP(S) URL should pass");
            assert_eq!(validated.target, BrowserNetworkTarget::Public);
            assert_eq!(validated.url, tauri::Url::parse(raw).unwrap());
        }
    }

    #[test]
    fn classifies_supported_loopback_hosts_without_dns() {
        for raw in [
            "http://localhost:5173",
            "http://app.localhost:3000/path",
            "http://127.42.0.1:8080",
            "http://[::1]:9000",
        ] {
            let validated = validate_browser_url(raw).expect("loopback HTTP URL should pass");
            assert_eq!(validated.target, BrowserNetworkTarget::Loopback, "{raw}");
        }
    }

    #[test]
    fn does_not_misclassify_lookalike_hosts_as_loopback() {
        for raw in [
            "https://localhost.example.com",
            "https://notlocalhost",
            "https://127.0.0.1.example.com",
        ] {
            let validated = validate_browser_url(raw).expect("valid public URL should pass");
            assert_eq!(validated.target, BrowserNetworkTarget::Public, "{raw}");
        }
    }

    #[test]
    fn rejects_relative_empty_malformed_and_control_character_urls() {
        for raw in [
            "",
            "/relative",
            "example.com",
            "http://",
            "http://example.com:bad",
            "https://example.com/\0hidden",
            "https://example.com/\nnext",
        ] {
            assert_eq!(
                validate_browser_url(raw).unwrap_err(),
                BrowserUrlError::InvalidUrl,
                "{raw:?}"
            );
        }
    }

    #[test]
    fn rejects_every_non_http_scheme() {
        for raw in [
            "file:///tmp/secret",
            "tauri://localhost",
            "data:text/html,hello",
            "javascript:alert(1)",
            "ftp://example.com/file",
        ] {
            assert_eq!(
                validate_browser_url(raw).unwrap_err(),
                BrowserUrlError::SchemeBlocked,
                "{raw}"
            );
        }
    }

    #[test]
    fn rejects_urls_with_embedded_credentials() {
        for raw in [
            "https://user@example.com",
            "https://user:pass@example.com",
            "https://:pass@example.com",
        ] {
            assert_eq!(
                validate_browser_url(raw).unwrap_err(),
                BrowserUrlError::CredentialsBlocked,
                "{raw}"
            );
        }
    }

    #[test]
    fn popup_and_download_policies_are_closed() {
        assert!(!allow_popup());
        assert!(!allow_download());
    }

    #[test]
    fn rejects_oversized_initial_and_page_authored_urls() {
        let oversized = format!("https://example.com/{}", "a".repeat(BROWSER_URL_BYTE_CAP));
        assert_eq!(
            validate_browser_url(&oversized).unwrap_err(),
            BrowserUrlError::InvalidUrl
        );

        let prefix = "https://example.com/";
        let at_cap = format!(
            "{prefix}{}",
            "a".repeat(BROWSER_URL_BYTE_CAP - prefix.len())
        );
        assert_eq!(at_cap.len(), BROWSER_URL_BYTE_CAP);
        assert!(validate_browser_url(&at_cap).is_ok());
    }
}

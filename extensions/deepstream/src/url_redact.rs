//! URL credential redaction helper (Task 5.3).
//!
//! Used anywhere a stream source URL is logged or surfaced to the UI. Without
//! redaction, RTSP URLs like `rtsp://admin:secret@host/path` would leak
//! credentials into host logs, EventBus payloads, and frontend-facing JSON.
//!
//! The `url` crate is already a dependency (`url = "2"` in Cargo.toml).

use url::Url;

/// Redact credentials in a URL string.
///
/// Behavior:
/// - Parse via `Url::parse`. On parse error or non-URL input, return the input unchanged.
/// - If the URL has a non-empty username OR a non-empty password, replace the
///   userinfo with `***` (so the output looks like `rtsp://***@host/path`).
/// - If neither username nor password is set, return the input unchanged
///   (common case — no work to do).
/// - Empty input returns empty input.
///
/// # Implementation note
///
/// We manually reconstruct the redacted URL string rather than round-tripping
/// through `Url::set_username("***")` + `Url::as_str()` because the `url` crate
/// may URL-encode the `***` placeholder differently across versions
/// (`%2A%2A%2A` vs `***`). The plan's test assertions expect literal `***`,
/// so we build the string by hand: take the original scheme + `://`, insert
/// `***@` only if there was userinfo, then append everything after the `@`.
/// If parsing succeeds but the userinfo can't be cleanly split out (rare —
/// e.g., the `@` lives inside a path segment of an unusual scheme), we fall
/// back to returning the input unchanged.
pub fn redact_url(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }

    let parsed = match Url::parse(s) {
        Ok(u) => u,
        Err(_) => return s.to_string(),
    };

    let username = parsed.username();
    let has_password = parsed.password().is_some();

    // No creds → no redaction needed. Return the original string (preserves
    // any non-canonical formatting the caller may rely on, like percent-encoding).
    if username.is_empty() && !has_password {
        return s.to_string();
    }

    // Has credentials. Rebuild as `<scheme>://***@<authority-without-userinfo><path>?<query>#<fragment>`.
    // We reconstruct from parts because Url doesn't expose a "strip creds and re-serialize" API
    // that's stable across versions; as_str() would re-encode the `***` placeholder.
    let scheme = parsed.scheme();
    let after_scheme = &s[scheme.len() + "://".len()..];

    // Find the first `@` in the post-scheme portion — it terminates the userinfo.
    // (Per RFC 3986, `@` cannot appear before the userinfo in the authority.)
    match after_scheme.find('@') {
        Some(at_idx) => {
            let rest = &after_scheme[at_idx + 1..];
            format!("{scheme}://***@{rest}")
        }
        // Parsed successfully and reported userinfo, but no `@` in source string.
        // Shouldn't happen for valid URLs; fall back to original for safety.
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_user_and_password() {
        assert_eq!(
            redact_url("rtsp://admin:secret@host/path"),
            "rtsp://***@host/path"
        );
    }

    #[test]
    fn redacts_user_only() {
        assert_eq!(redact_url("rtsp://admin@host/path"), "rtsp://***@host/path");
    }

    #[test]
    fn no_creds_unchanged() {
        assert_eq!(redact_url("rtsp://host/path"), "rtsp://host/path");
    }

    #[test]
    fn non_url_unchanged() {
        assert_eq!(redact_url("not a url"), "not a url");
    }

    #[test]
    fn redacts_password_only() {
        // Empty user but password present — still has creds worth redacting.
        assert_eq!(redact_url("rtsp://:pass@host/path"), "rtsp://***@host/path");
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(redact_url(""), "");
    }

    #[test]
    fn preserves_query_and_fragment() {
        assert_eq!(
            redact_url("rtsp://user:pw@host:8554/path?key=val#frag"),
            "rtsp://***@host:8554/path?key=val#frag"
        );
    }

    #[test]
    fn http_url_redacted_too() {
        // Works for any scheme the `url` crate recognizes.
        assert_eq!(
            redact_url("http://api:token@example.com/v1/feed"),
            "http://***@example.com/v1/feed"
        );
    }
}

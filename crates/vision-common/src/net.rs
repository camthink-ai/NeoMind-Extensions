//! URL guard — SSRF protection for user-controllable URLs.
//!
//! Multiple extensions accept URLs from command parameters (stream sources,
//! service endpoints, image URLs). Without validation, these can be pointed
//! at internal network resources (cloud metadata endpoints, admin panels,
//! internal databases) — a Server-Side Request Forgery (SSRF) vector.
//!
//! # Usage
//!
//! ```rust,ignore
//! use vision_common::net::UrlGuard;
//!
//! // In a command handler:
//! UrlGuard::validate(&url)?;
//!
//! // With custom allowed schemes:
//! let guard = UrlGuard::new().allow_rtsp();
//! guard.validate(&url)?;
//! ```

use std::net::IpAddr;

/// SSRF guard for user-controllable URLs.
#[derive(Debug, Clone)]
pub struct UrlGuard {
    /// Allowed URL schemes (default: http, https).
    allowed_schemes: Vec<String>,
    /// Whether to allow private/internal network addresses.
    allow_private: bool,
    /// Whether to allow localhost/127.0.0.1/::1.
    allow_localhost: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum UrlError {
    #[error("URL scheme '{0}' not allowed (permitted: {1})")]
    SchemeNotAllowed(String, String),
    #[error("URL host is a private address ({0}) — internal network access refused")]
    PrivateAddress(String),
    #[error("URL host is localhost ({0}) — refused unless explicitly allowed")]
    Localhost(String),
    #[error("URL could not be parsed: {0}")]
    ParseFailed(String),
    #[error("URL has no host")]
    NoHost,
}

impl Default for UrlGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl UrlGuard {
    /// Default guard: http/https only, no private addresses, no localhost.
    pub fn new() -> Self {
        Self {
            allowed_schemes: vec!["http".into(), "https".into()],
            allow_private: false,
            allow_localhost: false,
        }
    }

    /// Allow rtsp:// scheme (for camera stream extensions).
    pub fn allow_rtsp(mut self) -> Self {
        self.allowed_schemes.push("rtsp".into());
        self
    }

    /// Allow rtmp:// scheme.
    pub fn allow_rtmp(mut self) -> Self {
        self.allowed_schemes.push("rtmp".into());
        self
    }

    /// Allow file:// scheme (for local file input — use with caution).
    pub fn allow_file(mut self) -> Self {
        self.allowed_schemes.push("file".into());
        self
    }

    /// Allow private network addresses (for extensions that legitimately
    /// connect to internal services, e.g. modbus to a local PLC).
    pub fn allow_private_networks(mut self) -> Self {
        self.allow_private = true;
        self
    }

    /// Allow localhost/loopback (for extensions connecting to local
    /// services, e.g. a local Python inference server).
    pub fn allow_localhost(mut self) -> Self {
        self.allow_localhost = true;
        self
    }

    /// Validate a URL against this guard's policy.
    pub fn validate(&self, url: &str) -> Result<(), UrlError> {
        let (scheme, host) = self.parse(url)?;

        // Scheme check
        if !self.allowed_schemes.contains(&scheme) {
            return Err(UrlError::SchemeNotAllowed(
                scheme,
                self.allowed_schemes.join(", "),
            ));
        }

        // file:// has no host to check
        if scheme == "file" {
            return Ok(());
        }

        // Host checks
        let host_str = host.ok_or(UrlError::NoHost)?;
        self.check_host(&host_str)?;

        Ok(())
    }

    /// Minimal URL parser — extracts scheme and host without external deps.
    /// Handles: scheme://host:port/path, scheme://[ipv6]:port/path
    fn parse(&self, url: &str) -> Result<(String, Option<String>), UrlError> {
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| UrlError::ParseFailed("no :// separator".into()))?;

        let scheme = scheme.to_lowercase();

        // Extract host (between :// and next /, :, or end)
        let host_part = rest
            .split('/')
            .next()
            .unwrap_or("");

        // Handle [ipv6] notation
        let host = if host_part.starts_with('[') {
            host_part
                .split(']')
                .next()
                .map(|h| h.trim_start_matches('[').to_string())
        } else {
            // Strip port
            let h = host_part.split(':').next().unwrap_or("");
            if h.is_empty() {
                None
            } else {
                Some(h.to_string())
            }
        };

        Ok((scheme, host))
    }

    fn check_host(&self, host: &str) -> Result<(), UrlError> {
        // Try parsing as an IP address first
        if let Ok(ip) = host.parse::<IpAddr>() {
            match ip {
                IpAddr::V4(v4) => {
                    if v4.is_loopback() && !self.allow_localhost {
                        return Err(UrlError::Localhost(host.to_string()));
                    }
                    if !self.allow_private && (v4.is_private() || v4.is_link_local() || v4.is_unspecified()) {
                        return Err(UrlError::PrivateAddress(host.to_string()));
                    }
                }
                IpAddr::V6(v6) => {
                    if v6.is_loopback() && !self.allow_localhost {
                        return Err(UrlError::Localhost(host.to_string()));
                    }
                    // is_unique_local is stable since 1.7
                    if !self.allow_private && (v6.is_unique_local() || v6.is_unicast_link_local()) {
                        return Err(UrlError::PrivateAddress(host.to_string()));
                    }
                }
            }
            return Ok(());
        }

        // DNS name checks
        let lower = host.to_lowercase();
        if !self.allow_localhost && (lower == "localhost" || lower == "127.0.0.1" || lower == "::1" || lower == "[::1]") {
            return Err(UrlError::Localhost(host.to_string()));
        }

        // Well-known internal names
        if !self.allow_private {
            let internal_names = [
                "metadata.google.internal",
                "metadata",
                "instance-data",
                "169.254.169.254",
            ];
            if internal_names.contains(&lower.as_str()) {
                return Err(UrlError::PrivateAddress(host.to_string()));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_public_https() {
        UrlGuard::new().validate("https://api.example.com/v1").unwrap();
        UrlGuard::new().validate("http://cdn.example.org/img.png").unwrap();
    }

    #[test]
    fn rejects_non_http_schemes() {
        let err = UrlGuard::new().validate("ftp://files.example.com").unwrap_err();
        assert!(err.to_string().contains("not allowed"));
        let err = UrlGuard::new().validate("file:///etc/passwd").unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn allows_rtsp_when_configured() {
        UrlGuard::new().allow_rtsp().validate("rtsp://camera.example.com:554/stream").unwrap();
        // Default guard rejects rtsp
        assert!(UrlGuard::new().validate("rtsp://camera.example.com:554").is_err());
    }

    #[test]
    fn rejects_private_ipv4() {
        for addr in ["10.0.0.1", "192.168.1.1", "172.16.0.1", "169.254.169.254"] {
            let err = UrlGuard::new()
                .validate(&format!("http://{addr}/api"))
                .unwrap_err();
            assert!(err.to_string().contains("private"), "{addr} should be rejected");
        }
    }

    #[test]
    fn allows_private_when_configured() {
        UrlGuard::new()
            .allow_private_networks()
            .validate("http://10.0.0.5:502/modbus")
            .unwrap();
    }

    #[test]
    fn rejects_localhost_by_default() {
        let err = UrlGuard::new().validate("http://localhost:8080/api").unwrap_err();
        assert!(err.to_string().contains("localhost"));
        let err = UrlGuard::new().validate("http://127.0.0.1:9375/api").unwrap_err();
        assert!(err.to_string().contains("localhost") || err.to_string().contains("private"));
    }

    #[test]
    fn allows_localhost_when_configured() {
        // For local Python inference servers
        UrlGuard::new()
            .allow_localhost()
            .validate("http://127.0.0.1:8000/predict")
            .unwrap();
    }

    #[test]
    fn rejects_cloud_metadata() {
        let err = UrlGuard::new()
            .validate("http://metadata.google.internal/computeMetadata/v1/")
            .unwrap_err();
        assert!(err.to_string().contains("private") || err.to_string().contains("not allowed"));
    }

    #[test]
    fn handles_ipv6() {
        UrlGuard::new().validate("http://[2001:db8::1]:8080/api").unwrap();
        assert!(UrlGuard::new().validate("http://[::1]:8080").is_err());
        assert!(UrlGuard::new().validate("http://[fe80::1]:8080").is_err());
    }

    #[test]
    fn handles_port_and_path() {
        UrlGuard::new().validate("https://api.example.com:8443/v1/detect?x=1").unwrap();
        UrlGuard::new().validate("http://example.com:80/path/to/file.jpg").unwrap();
    }

    #[test]
    fn malformed_urls_error_cleanly() {
        assert!(UrlGuard::new().validate("not-a-url").is_err());
        assert!(UrlGuard::new().validate("http://").is_err());
        assert!(UrlGuard::new().validate("://missing-scheme").is_err());
    }

    #[test]
    fn local_inference_server_pattern() {
        // The common case: extension talking to its own local Python service
        let guard = UrlGuard::new().allow_localhost();
        guard.validate("http://127.0.0.1:8000/asr").unwrap();
        guard.validate("http://localhost:8000/asr").unwrap();
        // But still rejects private network
        assert!(guard.validate("http://10.0.0.1:8080").is_err());
    }
}

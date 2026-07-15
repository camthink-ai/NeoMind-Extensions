// ne503.rs — NE503 REST client (sync ureq).
//
// The NE503 firmware serves a SELF-SIGNED cert over HTTPS 443. To talk to it we
// build a rustls ClientConfig whose ServerCertVerifier accepts any cert, then
// hand that to ureq. NEVER use async HTTP here — this crate is a cdylib loaded
// into the host process and an embedded Tokio runtime would conflict.
use crate::config::Config;
use std::sync::Arc;

/// Sync REST client for the NE503 device. Holds a ureq Agent (with TLS-skip when
/// configured) and a login token behind a `parking_lot::RwLock`.
pub struct Ne503Client {
    base: String,
    agent: ureq::Agent,
    token: parking_lot::RwLock<Option<String>>,
    user: String,
    pass: String,
}

#[derive(Debug, thiserror::Error)]
pub enum NeError {
    #[error("http {0}")]
    Http(#[from] ureq::Error),
    #[error("io {0}")]
    Io(#[from] std::io::Error),
    #[error("bad status {0}: {1}")]
    Status(u16, String),
    #[error("no token")]
    NoToken,
    #[error("tls: {0}")]
    Tls(String),
}

impl Ne503Client {
    pub fn new(cfg: &Config) -> Self {
        Self {
            base: cfg.rest_base(), // https://<host>
            agent: build_insecure_agent(cfg.device.tls_insecure),
            token: Default::default(),
            user: cfg.device.username.clone(),
            pass: cfg.device.password.clone(),
        }
    }

    /// Pure, unit-testable: extract the token from a login JSON response.
    /// NE503 returns `{"code":0,"data":{"token":"Bearer ...","username":"admin"}}`.
    pub fn extract_token(resp: &serde_json::Value) -> Result<String, NeError> {
        resp["data"]["token"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| NeError::Status(200, "no token in response".into()))
    }

    /// `POST /api/login` with username/password; stores the returned token.
    pub fn login(&self) -> Result<(), NeError> {
        let resp: serde_json::Value = self
            .agent
            .post(&format!("{}/api/login", self.base))
            .send_json(serde_json::json!({"username": self.user, "password": self.pass }))?
            .into_json()?;
        *self.token.write() = Some(Self::extract_token(&resp)?);
        Ok(())
    }

    /// Return a valid token, logging in first if we don't have one yet.
    ///
    /// NOTE: on this firmware the token is static (`aipc-secure-token-secret`) so
    /// it won't expire, but in general a cached token can be revoked mid-session.
    /// TODO(P2): on a 401 from `get_device_status`, clear the token + `login()` +
    /// retry once so the WS ingest `?token=` (Task 8) doesn't inherit a dead token.
    fn authed(&self) -> Result<String, NeError> {
        match self.token.read().clone() {
            Some(t) => Ok(t),
            None => {
                self.login()?;
                // login() Ok ⟹ token is Some (the write is skipped if extract_token? fails).
                Ok(self.token.read().clone().ok_or(NeError::NoToken)?)
            }
        }
    }

    /// Current login token (already includes the `Bearer ` prefix on this
    /// firmware, so it is passed verbatim as the `Authorization` header value).
    /// `None` if not logged in. Used by the WS ingest `?token=` query param.
    pub fn token_string(&self) -> Option<String> {
        self.token.read().clone()
    }

    /// `GET /api/v1/device/status` → JSON `{"code":0,...}`.
    pub fn get_device_status(&self) -> Result<serde_json::Value, NeError> {
        let tok = self.authed()?;
        let resp = self
            .agent
            .get(&format!("{}/api/v1/device/status", self.base))
            .set("Authorization", &tok)
            .call()?;
        if resp.status() != 200 {
            return Err(NeError::Status(
                resp.status() as u16,
                resp.into_string().unwrap_or_default(),
            ));
        }
        Ok(resp.into_json()?)
    }

    /// P1: no single-frame REST endpoint on this firmware. P2 will grab a frame
    /// via ffmpeg one-shot RTSP (`rtsp://<host>:8554/sub`) on the host.
    pub fn get_snapshot(&self, _stream: &str) -> Result<String, NeError> {
        Err(NeError::Status(
            501,
            "snapshot not implemented until P2 (use RTSP ffmpeg one-shot)".into(),
        ))
    }
}

/// Build a ureq Agent. When `tls_insecure`, configure rustls with a
/// `ServerCertVerifier` that accepts any cert (the NE503 self-signed cert).
fn build_insecure_agent(tls_insecure: bool) -> ureq::Agent {
    if !tls_insecure {
        return ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(5))
            .build();
    }
    // rustls 0.23: in this workspace BOTH `ring` (via ureq) and `aws-lc-rs`
    // (via tokio-tungstenite) end up feature-unified onto rustls, so the
    // process-default CryptoProvider cannot be auto-detected. Explicitly select
    // the ring provider via builder_with_provider. Then install the NoVerify
    // ServerCertVerifier and hand the resulting ClientConfig to ureq.
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("ring provider supports safe default protocol versions")
    .dangerous()
    .with_custom_certificate_verifier(Arc::new(NoVerify))
    .with_no_client_auth();
    ureq::AgentBuilder::new()
        .tls_config(Arc::new(cfg))
        .timeout(std::time::Duration::from_secs(5))
        .build()
}

/// A `ServerCertVerifier` that accepts any certificate — for the NE503
/// self-signed cert only. Implements the rustls 0.23
/// `client::danger::ServerCertVerifier` trait: every verify method returns Ok,
/// and `supported_verify_schemes` advertises the full common scheme set.
#[derive(Debug)]
struct NoVerify;

impl rustls::client::danger::ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        // Advertise the full set of schemes rustls 0.23 knows about so the
        // server can pick whichever it prefers; we accept all signatures anyway.
        use rustls::SignatureScheme::*;
        vec![
            RSA_PKCS1_SHA256,
            ECDSA_NISTP256_SHA256,
            RSA_PKCS1_SHA384,
            ECDSA_NISTP384_SHA384,
            RSA_PKCS1_SHA512,
            ECDSA_NISTP521_SHA512,
            RSA_PSS_SHA256,
            RSA_PSS_SHA384,
            RSA_PSS_SHA512,
            ED25519,
            ED448,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_token_ok() {
        let resp = serde_json::json!({
            "code": 0,
            "data": {"token": "Bearer xyz", "username": "admin"}
        });
        let tok = Ne503Client::extract_token(&resp).unwrap();
        assert_eq!(tok, "Bearer xyz");
    }

    #[test]
    fn extract_token_missing() {
        let resp = serde_json::json!({});
        assert!(Ne503Client::extract_token(&resp).is_err());
    }

    #[test]
    fn extract_token_no_token_field() {
        // e.g. wrong-password response: code != 0, no data.token
        let resp = serde_json::json!({"code": 1, "msg": "bad password"});
        assert!(Ne503Client::extract_token(&resp).is_err());
    }

    /// LIVE integration test against the real NE503 at 192.168.93.200.
    /// Self-signed cert → proves TLS-skip works; login → proves token extract;
    /// get_device_status → proves the Bearer header round-trips.
    /// Run manually: `cargo test -p gym-tracker ne503::tests -- --ignored`.
    #[test]
    #[ignore]
    fn live_login_and_status() {
        let raw = r#"{"device":{"host":"192.168.93.200","username":"admin","password":"password","tls_insecure":true},"device_id":"ne503-001","ingest":{"topic":"gym/track","publish_hz":8,"track_ttl_sec":30,"reconnect_backoff_sec":[1,2,5,10,30]},"identity":{"match_threshold":0.55,"auto_capture_unknown":true,"unknown_prefix":"未知会员"},"roi":{"dwell_debounce_sec":3,"hysteresis":true},"data_dir":"/tmp/gym"}"#;
        let cfg = Config::parse(raw).expect("config parse");
        let client = Ne503Client::new(&cfg);

        client.login().expect("login should succeed");
        let tok = client.token_string().expect("token should be set after login");
        assert!(
            tok.starts_with("Bearer "),
            "token should start with 'Bearer ', was: {tok}"
        );

        let status = client.get_device_status().expect("device status should succeed");
        assert_eq!(
            status["code"].as_i64(),
            Some(0),
            "device status code should be 0, got: {status}"
        );
        eprintln!("live_login_and_status OK — token={tok}, status={status}");
    }
}

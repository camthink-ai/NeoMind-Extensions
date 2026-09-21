// tls.rs — shared rustls TLS-skip helpers.
//
// The NE503 firmware serves a SELF-SIGNED cert over HTTPS 443 and WSS 443. Both
// the sync REST client (ne503.rs, via ureq) and the async WS subscriber
// (ingest.rs, via tokio-tungstenite) need a `rustls::ClientConfig` whose
// `ServerCertVerifier` accepts any cert. This module owns that single source of
// truth so the two call sites can't drift.
//
// rustls 0.23: in this workspace BOTH `ring` (via ureq) and `aws-lc-rs`
// (via tokio-tungstenite) end up feature-unified onto rustls, so the
// process-default CryptoProvider cannot be auto-detected (`ClientConfig::builder()`
// panics at runtime). Explicitly select the `ring` provider via
// `builder_with_provider` — this is the proven workaround from ne503.rs.

use std::sync::Arc;

/// Build a `rustls::ClientConfig` whose certificate verifier accepts anything.
/// Hand the result to `ureq::AgentBuilder::tls_config(Arc::new(cfg))` (REST) or
/// `tokio_tungstenite::Connector::Rustls(Arc::new(cfg))` (WS).
pub fn insecure_client_config() -> rustls::ClientConfig {
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("ring provider supports safe default protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerify))
        .with_no_client_auth()
}

/// A `ServerCertVerifier` that accepts any certificate — for the NE503
/// self-signed cert only. Implements the rustls 0.23
/// `client::danger::ServerCertVerifier` trait: every verify method returns Ok,
/// and `supported_verify_schemes` advertises the full common scheme set.
#[derive(Debug)]
pub struct NoVerify;

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

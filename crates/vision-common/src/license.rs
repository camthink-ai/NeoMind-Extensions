//! Offline license verification — the commercial gating skeleton.
//!
//! Policy (deliberate, keep it this way):
//! - **No license file present ⇒ default open.** Every feature allowed,
//!   `source: DefaultOpen`. Keeps the free/trial story simple; the product
//!   can ship publicly before monetization is switched on.
//! - **License present + valid ⇒ features per the license** (task bits,
//!   expiry, optional device-fingerprint binding).
//! - **License present but tampered/expired/wrong-fingerprint ⇒ locked.**
//!   Fail-closed once licensing is engaged — an invalid key must never
//!   degrade into "default open".
//!
//! Format (single line, ASCII):
//! `NP1.<base64url(payload_json)>.<base64url(ed25519_signature)>`
//!
//! payload: `{"to": str, "exp": unix_secs | null, "features": ["detect",
//! "ocr", "face", "ground", "vlm", "*"], "fp": fingerprint | null}`
//!
//! The signature is Ed25519 over the payload bytes. The vendor keeps the
//! signing key; only the public key is embedded here (see `PRO_PUBLIC_KEY`
//! — placeholder all-zeros until the commercial keypair is generated; while
//! the placeholder is in place `verify` treats any signature as invalid,
//! which still fails closed, and default-open keeps everything usable).
//!
//! Verification is fully offline: no network, no phone-home, no telemetry.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Placeholder vendor public key. Replace with the real Ed25519 public key
/// when the commercial keypair is generated (see COMMERCIAL.md).
pub const PRO_PUBLIC_KEY: &[u8; 32] = &[0u8; 32];

/// Where licenses are searched, in order.
const LICENSE_ENV: &str = "NEOMIND_LICENSE_KEY";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LicenseSource {
    /// No license found — everything allowed (free/trial mode).
    DefaultOpen,
    /// Loaded and signature-verified.
    Verified,
    /// Present but rejected; everything denied. Carries the reason.
    Rejected(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseState {
    pub licensed_to: Option<String>,
    pub expires_at: Option<i64>,
    /// Task kinds granted; `["*"]` = all.
    pub features: Vec<String>,
    /// Device fingerprint the key is bound to, if any.
    pub fingerprint: Option<String>,
    pub source: LicenseSource,
}

impl Default for LicenseState {
    fn default() -> Self {
        Self::default_open()
    }
}

impl LicenseState {
    pub fn default_open() -> Self {
        Self {
            licensed_to: None,
            expires_at: None,
            features: vec!["*".to_string()],
            fingerprint: None,
            source: LicenseSource::DefaultOpen,
        }
    }

    /// Load from env / `<extension dir>/license.key`. Never panics; any
    /// problem degrades to DefaultOpen (absent) or Rejected (present but
    /// bad), per the policy above.
    pub fn load() -> Self {
        let read_file = |dir_env: &str| {
            std::env::var(dir_env)
                .ok()
                .map(|d| std::path::PathBuf::from(d).join("license.key"))
                .filter(|p| p.exists())
                .and_then(|p| std::fs::read_to_string(p).ok())
                .map(|s| s.trim().to_string())
        };
        let raw = std::env::var(LICENSE_ENV)
            .ok()
            // Platform-guaranteed data dir first (survives upgrades AND
            // uninstall — a license must outlive a reinstall), then the
            // legacy extension-root location for older platforms.
            .or_else(|| read_file("NEOMIND_EXTENSION_DATA_DIR"))
            .or_else(|| read_file("NEOMIND_EXTENSION_DIR"))
            .filter(|s| !s.is_empty());
        match raw {
            None => Self::default_open(),
            Some(raw) => Self::verify(&raw, PRO_PUBLIC_KEY),
        }
    }

    /// Verify a raw license line against a public key. Test hook — runtime
    /// callers use [`load`].
    pub fn verify(raw: &str, public_key: &[u8; 32]) -> Self {
        let fail = |reason: &str| Self {
            licensed_to: None,
            expires_at: None,
            features: vec![],
            fingerprint: None,
            source: LicenseSource::Rejected(reason.to_string()),
        };
        let Some(rest) = raw.trim().strip_prefix("NP1.") else {
            return fail("malformed license line");
        };
        let parts: Vec<&str> = rest.split('.').collect();
        if parts.len() != 2 {
            return fail("malformed license line");
        }
        let (payload_b64, sig_b64) = (parts[0], parts[1]);

        use base64::Engine;
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let payload = match engine.decode(payload_b64) {
            Ok(p) => p,
            Err(_) => return fail("payload not base64"),
        };
        let sig = match engine.decode(sig_b64) {
            Ok(s) => s,
            Err(_) => return fail("signature not base64"),
        };

        // Signature check
        #[cfg(not(target_arch = "wasm32"))]
        {
            use ed25519_dalek::{Signature, VerifyingKey};
            let Ok(vk) = VerifyingKey::from_bytes(public_key) else {
                return fail("bad vendor public key");
            };
            let Ok(sig) = Signature::from_slice(&sig) else {
                return fail("signature length");
            };
            if vk.verify_strict(&payload, &sig).is_err() {
                return fail("signature mismatch");
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            // No ed25519 verification on the wasm path. Fail CLOSED: a
            // present license that cannot be verified must not degrade
            // into "trusted" — that would break the module's core policy.
            let _ = (payload, sig);
            return fail("license verification unsupported on wasm");
        }

        // Payload check
        let parsed: LicensePayload = match serde_json::from_slice(&payload) {
            Ok(p) => p,
            Err(_) => return fail("payload not valid JSON"),
        };
        if let Some(exp) = parsed.exp {
            let now = chrono::Utc::now().timestamp();
            if now >= exp {
                return fail("license expired");
            }
        }
        if let Some(fp) = &parsed.fp {
            if fp != &device_fingerprint() {
                return fail("license bound to another device");
            }
        }

        Self {
            licensed_to: parsed.to,
            expires_at: parsed.exp,
            features: parsed.features,
            fingerprint: parsed.fp,
            source: LicenseSource::Verified,
        }
    }

    /// May `feature` (a task kind string) run?
    pub fn allows(&self, feature: &str) -> bool {
        match &self.source {
            LicenseSource::DefaultOpen | LicenseSource::Verified => {
                self.features.iter().any(|f| f == "*" || f == feature)
            }
            LicenseSource::Rejected(_) => false,
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match &self.source {
            LicenseSource::Rejected(r) => Some(r),
            _ => None,
        }
    }

    /// Uniform gate for task/feature entry points — the ONE helper
    /// extensions should call so every denial produces the same error
    /// shape (and nothing hand-rolls an `allows` check that forgets the
    /// fail-closed semantics).
    pub fn require(&self, feature: &str) -> Result<(), String> {
        if self.allows(feature) {
            return Ok(());
        }
        Err(match self.reason() {
            Some(r) => format!(
                "feature '{feature}' blocked by license: {r} — check                  license.key or contact support"
            ),
            None => format!("feature '{feature}' not included in this license"),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct LicensePayload {
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    exp: Option<i64>,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default)]
    fp: Option<String>,
}

/// Stable, privacy-friendly device fingerprint: first 16 hex of SHA-256
/// over the platform machine ID (never the raw ID itself).
pub fn device_fingerprint() -> String {
    // Cached: probes spawn `ioreg` on macOS (tens of ms) and the value is
    // stable for the process lifetime.
    static FP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if let Some(cached) = FP.get() {
        return cached.clone();
    }
    let computed = compute_device_fingerprint();
    let _ = FP.set(computed.clone());
    computed
}

fn compute_device_fingerprint() -> String {
    let machine_id = platform_machine_id().unwrap_or_else(|| {
        std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| "unknown-host".to_string())
    });
    let mut h = Sha256::new();
    h.update(b"neomind-pro-fp-v1:");
    h.update(machine_id.as_bytes());
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    hex[..16].to_string()
}

fn platform_machine_id() -> Option<String> {
    for p in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(id) = std::fs::read_to_string(p) {
            let id = id.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        let line = s.lines().find(|l| l.contains("IOPlatformUUID"))?;
        let uuid = line.split('"').nth(3)?.to_string();
        if !uuid.is_empty() {
            return Some(uuid);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};

    fn issue(sk: &SigningKey, payload: &serde_json::Value) -> String {
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let bytes = serde_json::to_vec(payload).unwrap();
        let sig = sk.sign(&bytes);
        format!(
            "NP1.{}.{}",
            engine.encode(&bytes),
            engine.encode(sig.to_bytes())
        )
    }

    fn keys() -> (SigningKey, [u8; 32]) {
        let sk = SigningKey::from_bytes(&[7u8; 32]); // deterministic test key
        let pk = sk.verifying_key().to_bytes();
        (sk, pk)
    }

    #[test]
    fn default_open_allows_everything() {
        let l = LicenseState::default_open();
        assert!(l.allows("detect"));
        assert!(l.allows("vlm"));
        assert_eq!(l.source, LicenseSource::DefaultOpen);
    }

    #[test]
    fn valid_license_grants_listed_features() {
        let (sk, pk) = keys();
        let raw = issue(
            &sk,
            &serde_json::json!({"to": "acme", "features": ["detect", "ocr"]}),
        );
        let l = LicenseState::verify(&raw, &pk);
        assert_eq!(l.source, LicenseSource::Verified);
        assert!(l.allows("detect"));
        assert!(l.allows("ocr"));
        assert!(!l.allows("face"));
        assert_eq!(l.licensed_to.as_deref(), Some("acme"));
    }

    #[test]
    fn wildcard_grants_all() {
        let (sk, pk) = keys();
        let raw = issue(&sk, &serde_json::json!({"features": ["*"]}));
        let l = LicenseState::verify(&raw, &pk);
        assert!(l.allows("face"));
    }

    #[test]
    fn tampered_payload_rejected_closed() {
        let (sk, pk) = keys();
        let raw = issue(&sk, &serde_json::json!({"features": ["detect"]}));
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        // re-encode a DIFFERENT payload under the same signature
        let evil = serde_json::json!({"features": ["*"]});
        let evil_b64 = engine.encode(serde_json::to_vec(&evil).unwrap());
        let sig_b64 = raw.split('.').nth(2).unwrap();
        let tampered = format!("NP1.{evil_b64}.{sig_b64}");
        let l = LicenseState::verify(&tampered, &pk);
        assert!(matches!(l.source, LicenseSource::Rejected(_)));
        assert!(!l.allows("detect"), "rejected license must fail closed");
    }

    #[test]
    fn wrong_key_rejected() {
        let (sk, _) = keys();
        let other = SigningKey::from_bytes(&[9u8; 32]);
        let raw = issue(&sk, &serde_json::json!({"features": ["*"]}));
        let l = LicenseState::verify(&raw, &other.verifying_key().to_bytes());
        assert!(matches!(l.source, LicenseSource::Rejected(_)));
    }

    #[test]
    fn expired_rejected() {
        let (sk, pk) = keys();
        let past = chrono::Utc::now().timestamp() - 100;
        let raw = issue(&sk, &serde_json::json!({"features": ["*"], "exp": past}));
        let l = LicenseState::verify(&raw, &pk);
        assert!(matches!(l.source, LicenseSource::Rejected(r) if r.contains("expired")));
    }

    #[test]
    fn fingerprint_mismatch_rejected() {
        let (sk, pk) = keys();
        let raw = issue(
            &sk,
            &serde_json::json!({"features": ["*"], "fp": "0000000000000000"}),
        );
        let l = LicenseState::verify(&raw, &pk);
        assert!(matches!(l.source, LicenseSource::Rejected(r) if r.contains("another device")));
    }

    #[test]
    fn malformed_lines_rejected() {
        let (_, pk) = keys();
        for bad in ["", "garbage", "NP1.onlyonepart", "NP1.!!.??"] {
            let l = LicenseState::verify(bad, &pk);
            assert!(matches!(l.source, LicenseSource::Rejected(_)), "line: {bad}");
        }
    }

    #[test]
    fn require_matches_allows_semantics() {
        let l = LicenseState::default_open();
        assert!(l.require("detect").is_ok());
        let rejected = LicenseState {
            licensed_to: None,
            expires_at: None,
            features: vec![],
            fingerprint: None,
            source: LicenseSource::Rejected("test".into()),
        };
        let err = rejected.require("detect").unwrap_err();
        assert!(err.contains("blocked by license"));
        let partial = LicenseState {
            licensed_to: None,
            expires_at: None,
            features: vec!["detect".into()],
            fingerprint: None,
            source: LicenseSource::Verified,
        };
        assert!(partial.require("detect").is_ok());
        assert!(partial.require("face").unwrap_err().contains("not included"));
    }

    #[test]
        fn fingerprint_is_stable_and_hashed() {
        let a = device_fingerprint();
        let b = device_fingerprint();
        assert_eq!(a, b, "fingerprint must be stable within a process");
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

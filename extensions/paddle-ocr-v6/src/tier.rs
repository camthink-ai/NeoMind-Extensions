//! Tier selection for PP-OCRv6 models.
//!
//! PP-OCRv6 ships three tiers — tiny (1.7+4.3 MB), small, medium —
//! trading accuracy for footprint. Auto tier selects based on local
//! hardware capability reported by the caller (this module does NOT
//! probe hardware itself; it's pure logic, fully testable).

use neomind_extension_sdk::ExtensionError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Tiny,
    Small,
    Medium,
    Auto,
}

impl Tier {
    /// Parse from a config string. Case-insensitive.
    pub fn from_str(s: &str) -> Result<Self, ExtensionError> {
        match s.to_lowercase().as_str() {
            "tiny" => Ok(Tier::Tiny),
            "small" => Ok(Tier::Small),
            "medium" => Ok(Tier::Medium),
            "auto" => Ok(Tier::Auto),
            _ => Err(ExtensionError::InvalidArguments(format!(
                "Unknown tier: '{}'. Expected: tiny|small|medium|auto",
                s
            ))),
        }
    }

    /// Resolve `Auto` to a concrete tier based on host capability.
    /// Explicit tiers pass through unchanged.
    ///
    /// - CUDA + ≥16 GB RAM → Medium
    /// - CUDA or CoreML (regardless of RAM) → Small
    /// - otherwise → Tiny
    pub fn resolve(self, has_cuda: bool, has_coreml: bool, ram_gb: u64) -> Tier {
        match self {
            Tier::Tiny | Tier::Small | Tier::Medium => self,
            Tier::Auto => {
                if has_cuda && ram_gb >= 16 {
                    Tier::Medium
                } else if has_cuda || has_coreml {
                    Tier::Small
                } else {
                    Tier::Tiny
                }
            }
        }
    }

    /// Filename segment used in model file names: "tiny" / "small" / "medium".
    /// Panics on Auto — caller must call `resolve()` first.
    pub fn filename_segment(&self) -> &'static str {
        match self {
            Tier::Tiny => "tiny",
            Tier::Small => "small",
            Tier::Medium => "medium",
            Tier::Auto => panic!("Tier::Auto has no filename; call resolve() first"),
        }
    }

    /// Display string for metrics / config echo.
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::Tiny => "tiny",
            Tier::Small => "small",
            Tier::Medium => "medium",
            Tier::Auto => "auto",
        }
    }
}

impl Default for Tier {
    fn default() -> Self {
        Tier::Auto
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_valid() {
        assert_eq!(Tier::from_str("tiny").unwrap(), Tier::Tiny);
        assert_eq!(Tier::from_str("SMALL").unwrap(), Tier::Small);
        assert_eq!(Tier::from_str("Medium").unwrap(), Tier::Medium);
        assert_eq!(Tier::from_str("auto").unwrap(), Tier::Auto);
    }

    #[test]
    fn test_from_str_invalid() {
        assert!(Tier::from_str("huge").is_err());
        assert!(Tier::from_str("").is_err());
    }

    #[test]
    fn test_resolve_auto_cpu_only() {
        // CPU-only hosts always get Tiny regardless of RAM.
        assert_eq!(Tier::Auto.resolve(false, false, 4), Tier::Tiny);
        assert_eq!(Tier::Auto.resolve(false, false, 32), Tier::Tiny);
    }

    #[test]
    fn test_resolve_auto_coreml() {
        // CoreML (Apple Silicon) → Small.
        assert_eq!(Tier::Auto.resolve(false, true, 8), Tier::Small);
        assert_eq!(Tier::Auto.resolve(false, true, 32), Tier::Small);
    }

    #[test]
    fn test_resolve_auto_cuda() {
        // CUDA + <16 GB → Small; CUDA + ≥16 GB → Medium.
        assert_eq!(Tier::Auto.resolve(true, false, 8), Tier::Small);
        assert_eq!(Tier::Auto.resolve(true, false, 16), Tier::Medium);
        assert_eq!(Tier::Auto.resolve(true, false, 64), Tier::Medium);
    }

    #[test]
    fn test_resolve_explicit_passthrough() {
        // Explicit tiers ignore host capability entirely.
        assert_eq!(Tier::Tiny.resolve(true, true, 64), Tier::Tiny);
        assert_eq!(Tier::Medium.resolve(false, false, 2), Tier::Medium);
    }

    #[test]
    fn test_filename_segment() {
        assert_eq!(Tier::Tiny.filename_segment(), "tiny");
        assert_eq!(Tier::Small.filename_segment(), "small");
        assert_eq!(Tier::Medium.filename_segment(), "medium");
    }

    #[test]
    #[should_panic(expected = "Tier::Auto has no filename")]
    fn test_filename_segment_auto_panics() {
        let _ = Tier::Auto.filename_segment();
    }

    #[test]
    fn test_as_str_round_trip() {
        for s in &["tiny", "small", "medium", "auto"] {
            let t = Tier::from_str(s).unwrap();
            assert_eq!(t.as_str(), *s);
        }
    }
}

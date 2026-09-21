//! Model tier selection (ported from paddle-ocr-v6, itself derived from the
//! PP-OCR tiny/small/medium tiers). Pure logic — no hardware probing here.

use serde::{Deserialize, Serialize};

/// Accuracy/footprint tier for model families that ship multiple sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tier {
    Tiny,
    Small,
    Medium,
    Auto,
}

impl Tier {
    /// Parse from a config string. Case-insensitive.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "tiny" => Ok(Tier::Tiny),
            "small" => Ok(Tier::Small),
            "medium" => Ok(Tier::Medium),
            "auto" => Ok(Tier::Auto),
            _ => Err(format!("Unknown tier: '{s}'. Expected: tiny|small|medium|auto")),
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

    /// Filename segment: "tiny" / "small" / "medium".
    pub fn filename_segment(&self) -> &'static str {
        match self {
            Tier::Tiny => "tiny",
            Tier::Small => "small",
            Tier::Medium => "medium",
            Tier::Auto => "auto",
        }
    }

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

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid() {
        assert_eq!(Tier::parse("tiny").unwrap(), Tier::Tiny);
        assert_eq!(Tier::parse("SMALL").unwrap(), Tier::Small);
        assert_eq!(Tier::parse("Medium").unwrap(), Tier::Medium);
        assert_eq!(Tier::parse("auto").unwrap(), Tier::Auto);
    }

    #[test]
    fn parse_invalid() {
        assert!(Tier::parse("huge").is_err());
        assert!(Tier::parse("").is_err());
    }

    #[test]
    fn resolve_auto_cpu_only() {
        assert_eq!(Tier::Auto.resolve(false, false, 4), Tier::Tiny);
        assert_eq!(Tier::Auto.resolve(false, false, 32), Tier::Tiny);
    }

    #[test]
    fn resolve_auto_coreml() {
        assert_eq!(Tier::Auto.resolve(false, true, 8), Tier::Small);
        assert_eq!(Tier::Auto.resolve(false, true, 32), Tier::Small);
    }

    #[test]
    fn resolve_auto_cuda() {
        assert_eq!(Tier::Auto.resolve(true, false, 8), Tier::Small);
        assert_eq!(Tier::Auto.resolve(true, false, 16), Tier::Medium);
        assert_eq!(Tier::Auto.resolve(true, false, 64), Tier::Medium);
    }

    #[test]
    fn resolve_explicit_passthrough() {
        assert_eq!(Tier::Tiny.resolve(true, true, 64), Tier::Tiny);
        assert_eq!(Tier::Medium.resolve(false, false, 2), Tier::Medium);
    }
}

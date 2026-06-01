use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaConfig {
    pub ha_url: String,
    pub token: String,
    #[serde(default = "default_domains")]
    pub domains: Vec<String>,
    #[serde(default = "default_sync_interval")]
    pub sync_interval: u64,
    #[serde(default)]
    pub entity_patterns: Vec<String>,
}

fn default_domains() -> Vec<String> {
    vec![
        "sensor".into(),
        "light".into(),
        "switch".into(),
        "climate".into(),
        "lock".into(),
    ]
}

fn default_sync_interval() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaEntity {
    pub entity_id: String,
    pub domain: String,
    pub name: String,
    pub state: String,
    pub value: Option<f64>,
    pub unit: Option<String>,
    pub battery: Option<u8>,
    pub last_changed: String,
    /// Key attributes from HA (brightness, hvac_mode, device_class, etc.)
    pub attributes: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct HaStateResponse {
    pub entity_id: String,
    pub state: String,
    pub attributes: serde_json::Value,
    pub last_changed: String,
}

impl HaStateResponse {
    pub fn to_entity(&self) -> HaEntity {
        let domain = self
            .entity_id
            .split('.')
            .next()
            .unwrap_or("")
            .to_string();
        let name = self
            .attributes
            .get("friendly_name")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.entity_id)
            .to_string();
        let value = self.state.parse::<f64>().ok();
        let unit = self
            .attributes
            .get("unit_of_measurement")
            .and_then(|v| v.as_str())
            .map(String::from);
        let battery = self
            .attributes
            .get("battery")
            .or_else(|| self.attributes.get("battery_level"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u8);

        // Preserve key attributes that are useful for control and display.
        let attributes = extract_key_attributes(&self.attributes);

        HaEntity {
            entity_id: self.entity_id.clone(),
            domain,
            name,
            state: self.state.clone(),
            value,
            unit,
            battery,
            last_changed: self.last_changed.clone(),
            attributes,
        }
    }
}

/// Extract key attributes from the HA attributes JSON object.
/// These are the most commonly needed attributes for controlling devices
/// and displaying meaningful information.
fn extract_key_attributes(attrs: &serde_json::Value) -> HashMap<String, serde_json::Value> {
    let mut result = HashMap::new();

    // Key attributes to preserve (when present)
    let keys = [
        "device_class",
        "friendly_name",
        "unit_of_measurement",
        "brightness",
        "color_mode",
        "color_temp",
        "min_mireds",
        "max_mireds",
        "hvac_mode",
        "hvac_action",
        "hvac_modes",
        "target_temp",
        "current_temperature",
        "temperature_unit",
        "fan_mode",
        "fan_modes",
        "preset_mode",
        "preset_modes",
        "swing_mode",
        "swing_modes",
        "supported_features",
        "supported_color_modes",
        "effect",
        "effect_list",
        "icon",
        "assumed_state",
        "brightness_step",
        "max_brightness",
        "min_brightness",
        "position",
        "current_position",
        "is_closed",
        "locked",
        "code_format",
        "volume_level",
        "media_title",
        "media_artist",
        "media_duration",
        "media_position",
        "source",
        "source_list",
        "sound_mode",
        "sound_mode_list",
        "options",
        "option",
    ];

    if let Some(map) = attrs.as_object() {
        for key in &keys {
            if let Some(val) = map.get(*key) {
                if !val.is_null() {
                    result.insert(key.to_string(), val.clone());
                }
            }
        }
    }

    result
}

/// Check whether an entity_id matches any of the given patterns.
///
/// Patterns support simple glob matching:
/// - `*` matches any sequence of characters
/// - A pattern without wildcards acts as a substring/contains match
/// - If patterns is empty, all entities match (accept all)
pub fn entity_matches_patterns(entity_id: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return true;
    }
    let entity_lower = entity_id.to_lowercase();
    for pattern in patterns {
        let pattern_lower = pattern.to_lowercase();
        if pattern_lower.contains('*') {
            // Simple glob: split by '*' and check all parts appear in order
            let parts: Vec<&str> = pattern_lower.split('*').filter(|s| !s.is_empty()).collect();
            if parts.is_empty() {
                // Pattern was just "*" or "**" — matches everything
                return true;
            }
            let mut search_from = 0;
            let mut all_match = true;
            for part in &parts {
                if let Some(pos) = entity_lower[search_from..].find(part) {
                    search_from += pos + part.len();
                } else {
                    all_match = false;
                    break;
                }
            }
            if all_match {
                return true;
            }
        } else {
            // Substring match
            if entity_lower.contains(&pattern_lower) {
                return true;
            }
        }
    }
    false
}

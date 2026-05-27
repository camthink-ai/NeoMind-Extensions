use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaConfig {
    pub ha_url: String,
    pub token: String,
    #[serde(default = "default_domains")]
    pub domains: Vec<String>,
    #[serde(default = "default_sync_interval")]
    pub sync_interval: u64,
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

        HaEntity {
            entity_id: self.entity_id.clone(),
            domain,
            name,
            state: self.state.clone(),
            value,
            unit,
            battery,
            last_changed: self.last_changed.clone(),
        }
    }
}

#[cfg(test)]
pub fn domain_has_commands(domain: &str) -> bool {
    matches!(
        domain,
        "light"
            | "switch"
            | "climate"
            | "lock"
            | "cover"
            | "fan"
            | "media_player"
    )
}

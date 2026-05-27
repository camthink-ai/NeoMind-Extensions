use crate::types::{HaConfig, HaEntity, HaStateResponse};

/// Sync REST API wrapper for Home Assistant using ureq v3.
pub struct HaRestClient {
    base_url: String,
    token: String,
    agent: ureq::Agent,
}

impl HaRestClient {
    pub fn new(config: &HaConfig) -> Self {
        let base_url = config.ha_url.trim_end_matches('/').to_string();
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build()
            .into();
        Self {
            base_url,
            token: config.token.clone(),
            agent,
        }
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// Test connection to Home Assistant: GET /api/
    /// Returns the HA welcome message on success.
    pub fn test_connection(&self) -> Result<String, String> {
        let url = format!("{}/api/", self.base_url);
        let mut resp = self
            .agent
            .get(&url)
            .header("Authorization", &self.auth_header())
            .call()
            .map_err(|e| format!("Connection test failed: {}", e))?;

        let body: serde_json::Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("Failed to read response: {}", e))?;

        body
            .get("message")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| "Invalid HA API response: missing 'message' field".to_string())
    }

    /// Get all states filtered by the given domain list: GET /api/states
    pub fn get_all_states(&self, domains: &[String]) -> Result<Vec<HaEntity>, String> {
        let url = format!("{}/api/states", self.base_url);
        let mut resp = self
            .agent
            .get(&url)
            .header("Authorization", &self.auth_header())
            .call()
            .map_err(|e| format!("Failed to get states: {}", e))?;

        let states: Vec<HaStateResponse> = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("Failed to parse states: {}", e))?;

        let entities: Vec<HaEntity> = states
            .into_iter()
            .filter(|s| {
                let domain = s.entity_id.split('.').next().unwrap_or("");
                domains.is_empty() || domains.iter().any(|d| d == domain)
            })
            .map(|s| s.to_entity())
            .collect();

        Ok(entities)
    }

    /// Get a single entity state: GET /api/states/{entity_id}
    pub fn get_state(&self, entity_id: &str) -> Result<HaEntity, String> {
        let url = format!("{}/api/states/{}", self.base_url, entity_id);
        let mut resp = self
            .agent
            .get(&url)
            .header("Authorization", &self.auth_header())
            .call()
            .map_err(|e| format!("Failed to get state for '{}': {}", entity_id, e))?;

        let state: HaStateResponse = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("Failed to parse state for '{}': {}", entity_id, e))?;

        Ok(state.to_entity())
    }

    /// Call a service on Home Assistant: POST /api/services/{domain}/{service}
    ///
    /// `data` should contain `entity_id` and any additional service data.
    pub fn call_service(
        &self,
        domain: &str,
        service: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let url = format!("{}/api/services/{}/{}", self.base_url, domain, service);
        let mut resp = self
            .agent
            .post(&url)
            .header("Authorization", &self.auth_header())
            .header("Content-Type", "application/json")
            .send_json(data)
            .map_err(|e| format!("Failed to call service {}.{}: {}", domain, service, e))?;

        let result: serde_json::Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("Failed to parse service response: {}", e))?;

        Ok(result)
    }
}

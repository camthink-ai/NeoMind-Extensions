use crate::types::{HaConfig, HaEntity, HaStateResponse};

/// Sync REST API wrapper for Home Assistant using ureq v2.
#[derive(Clone)]
pub struct HaRestClient {
    base_url: String,
    token: String,
    agent: ureq::Agent,
}

impl HaRestClient {
    pub fn new(config: &HaConfig) -> Self {
        let base_url = config.ha_url.trim_end_matches('/').to_string();
        let agent = ureq::AgentBuilder::new()
            .timeout_read(std::time::Duration::from_secs(30))
            .timeout_write(std::time::Duration::from_secs(30))
            .build();
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
        let resp = self
            .agent
            .get(&url)
            .set("Authorization", &self.auth_header())
            .call()
            .map_err(|e| self.friendly_connection_error(&url, &e))?;

        let body: serde_json::Value = resp
            .into_json()
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
        let resp = self
            .agent
            .get(&url)
            .set("Authorization", &self.auth_header())
            .call()
            .map_err(|e| self.friendly_connection_error(&url, &e))?;

        let states: Vec<HaStateResponse> = resp
            .into_json()
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
        let resp = self
            .agent
            .get(&url)
            .set("Authorization", &self.auth_header())
            .call()
            .map_err(|e| format!("Failed to get state for '{}': {}", entity_id, e))?;

        let state: HaStateResponse = resp
            .into_json()
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
        let resp = self
            .agent
            .post(&url)
            .set("Authorization", &self.auth_header())
            .set("Content-Type", "application/json")
            .send_json(data)
            .map_err(|e| format!("Failed to call service {}.{}: {}", domain, service, e))?;

        let result: serde_json::Value = resp
            .into_json()
            .map_err(|e| format!("Failed to parse service response: {}", e))?;

        Ok(result)
    }

    /// Get all areas from Home Assistant.
    ///
    /// NOTE: Home Assistant does not expose areas via a public REST endpoint.
    /// The area registry is only accessible through the WebSocket API
    /// (command `get_areas`). This REST method attempts the undocumented
    /// `/api/config/area_registry/list` path and gracefully degrades if
    /// it returns 404 or another error.
    pub fn get_areas(&self) -> Result<serde_json::Value, String> {
        let url = format!("{}/api/config/area_registry/list", self.base_url);
        let resp = self
            .agent
            .get(&url)
            .set("Authorization", &self.auth_header())
            .call();

        match resp {
            Ok(resp) => {
                let areas: serde_json::Value = resp
                    .into_json()
                    .map_err(|e| format!("Failed to parse areas: {}", e))?;
                Ok(areas)
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("404") {
                    Err(
                        "Home Assistant does not expose areas via REST API. \
                         Areas are only available through the WebSocket API. \
                         This command is not supported with REST-only connections."
                            .to_string(),
                    )
                } else {
                    Err(format!("Failed to get areas: {}", e))
                }
            }
        }
    }

    /// Convert raw HTTP errors into user-friendly messages with actionable hints.
    fn friendly_connection_error(&self, url: &str, e: &ureq::Error) -> String {
        let err_str = e.to_string();
        if err_str.contains("401") || err_str.contains("403") {
            "Authentication failed: invalid token. Generate a new Long-Lived Access Token in Home Assistant (Profile > Security > Long-Lived Access Tokens)".to_string()
        } else if err_str.contains("connect") || err_str.contains("timeout") || err_str.contains("refused") {
            format!(
                "Cannot reach Home Assistant at {}. Check: 1) URL format (http://IP:8123), 2) HA is running, 3) Network/firewall allows connection",
                self.base_url
            )
        } else if err_str.contains("tls") || err_str.contains("certificate") || err_str.contains("ssl") {
            "TLS/SSL error. If your HA uses plain HTTP, change URL from https:// to http://".to_string()
        } else if err_str.contains("404") {
            format!("Home Assistant API not found at {}. Ensure HA is running and URL is correct (should end with :8123, not :80)", url)
        } else {
            format!("Connection to Home Assistant failed: {}. Check that HA is running and URL is correct", err_str)
        }
    }
}

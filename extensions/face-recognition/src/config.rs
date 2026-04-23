//! Configuration persistence and management for the face-recognition extension.

use crate::types::{BindingStats, FaceRecConfig};
use crate::FaceRecognition;

impl FaceRecognition {
    /// Get the current face database path based on NEOMIND_EXTENSION_DIR.
    pub fn get_faces_db_path() -> Option<std::path::PathBuf> {
        std::env::var("NEOMIND_EXTENSION_DIR")
            .ok()
            .map(|dir| std::path::PathBuf::from(dir).join("faces.json"))
    }

    /// Get the current config file path.
    pub fn get_config_path() -> Option<std::path::PathBuf> {
        std::env::var("NEOMIND_EXTENSION_DIR")
            .ok()
            .map(|dir| std::path::PathBuf::from(dir).join("config.json"))
    }

    /// Save face database to faces.json.
    pub fn save_face_database(&self) {
        if let Some(path) = Self::get_faces_db_path() {
            let db = self.face_db.read();
            if let Err(e) = db.save_to_file(&path) {
                tracing::warn!(
                    "[FaceRecognition] Failed to save face database: {}",
                    e
                );
            } else {
                tracing::debug!(
                    "[FaceRecognition] Face database saved to {}",
                    path.display()
                );
            }
        }
    }

    /// Persist configuration to config.json.
    pub fn persist_config(&self) {
        if let Some(path) = Self::get_config_path() {
            let config = self.config.read();
            let mut config_to_save = config.clone();
            config_to_save.bindings = self.bindings.read().values().cloned().collect();

            match serde_json::to_string_pretty(&config_to_save) {
                Ok(json_str) => {
                    if let Err(e) = std::fs::write(&path, json_str) {
                        tracing::warn!(
                            "[FaceRecognition] Failed to persist config: {}",
                            e
                        );
                    } else {
                        tracing::debug!(
                            "[FaceRecognition] Config persisted to {}",
                            path.display()
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "[FaceRecognition] Failed to serialize config: {}",
                        e
                    );
                }
            }
        }
    }

    /// Load configuration from config.json file.
    pub fn load_config_from_file(&self) -> Option<FaceRecConfig> {
        // Try current directory first (extension runner sets cwd)
        let config_path = std::path::PathBuf::from("config.json");
        if config_path.exists() {
            if let Ok(json_str) = std::fs::read_to_string(&config_path) {
                if let Ok(config) = serde_json::from_str::<FaceRecConfig>(&json_str) {
                    tracing::info!(
                        "[FaceRecognition] Loaded config from file with {} bindings",
                        config.bindings.len()
                    );
                    return Some(config);
                }
            }
        }

        // Try NEOMIND_EXTENSION_DIR as fallback
        if let Some(path) = Self::get_config_path() {
            if path.exists() {
                if let Ok(json_str) = std::fs::read_to_string(&path) {
                    if let Ok(config) = serde_json::from_str::<FaceRecConfig>(&json_str) {
                        tracing::info!(
                            "[FaceRecognition] Loaded config from {} with {} bindings",
                            path.display(),
                            config.bindings.len()
                        );
                        return Some(config);
                    }
                }
            }
        }

        None
    }

    /// Load persisted bindings from a config into the runtime state.
    pub fn restore_bindings(&self, config: &FaceRecConfig) {
        for binding in &config.bindings {
            self.bindings
                .write()
                .insert(binding.device_id.clone(), binding.clone());
            self.binding_stats.write().insert(
                binding.device_id.clone(),
                BindingStats {
                    total_inferences: 0,
                    total_recognized: 0,
                    total_unknown: 0,
                    last_image: None,
                    last_faces: None,
                    last_error: None,
                },
            );
            tracing::info!(
                "[FaceRecognition] Restored binding for device: {}",
                binding.device_id
            );
        }
    }
}

use parking_lot::RwLock;
use std::collections::HashMap;

use crate::types::{OpcUaNode, SubscriptionInfo};

/// Thread-safe node value cache for synchronous produce_metrics access.
pub struct NodeCache {
    nodes: RwLock<HashMap<String, OpcUaNode>>,
    subscriptions: RwLock<HashMap<String, SubscriptionInfo>>,
}

impl NodeCache {
    pub fn new() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
            subscriptions: RwLock::new(HashMap::new()),
        }
    }

    pub fn upsert_node(&self, node: OpcUaNode) {
        let mut nodes = self.nodes.write();
        if let Some(existing) = nodes.get_mut(&node.node_id) {
            // Preserve runtime-only fields from existing node
            existing.browse_name = node.browse_name;
            existing.display_name = node.display_name;
            existing.node_class = node.node_class;
            existing.data_type = node.data_type;
            existing.children = node.children;
            // Only overwrite value/quality/timestamp if explicitly provided
            if node.value.is_some() {
                existing.value = node.value;
            }
            if node.quality.is_some() {
                existing.quality = node.quality;
            }
            if node.source_timestamp.is_some() {
                existing.source_timestamp = node.source_timestamp;
            }
        } else {
            nodes.insert(node.node_id.clone(), node);
        }
    }

    pub fn get_node(&self, node_id: &str) -> Option<OpcUaNode> {
        self.nodes.read().get(node_id).cloned()
    }

    pub fn get_all_nodes(&self) -> Vec<OpcUaNode> {
        self.nodes.read().values().cloned().collect()
    }

    #[allow(dead_code)]
    pub fn remove_node(&self, node_id: &str) {
        self.nodes.write().remove(node_id);
    }

    pub fn clear_nodes(&self) {
        self.nodes.write().clear();
    }

    pub fn node_count(&self) -> usize {
        self.nodes.read().len()
    }

    pub fn update_node_value(
        &self,
        node_id: &str,
        value: serde_json::Value,
        quality: Option<String>,
        timestamp: Option<i64>,
    ) {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(node_id) {
            node.value = Some(value);
            node.quality = quality;
            node.source_timestamp = timestamp;
        }
    }

    pub fn upsert_subscription(&self, sub: SubscriptionInfo) {
        self.subscriptions
            .write()
            .insert(sub.subscription_id.clone(), sub);
    }

    pub fn remove_subscription(&self, sub_id: &str) {
        self.subscriptions.write().remove(sub_id);
    }

    pub fn get_all_subscriptions(&self) -> Vec<SubscriptionInfo> {
        self.subscriptions.read().values().cloned().collect()
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.read().len()
    }

    pub fn clear_subscriptions(&self) {
        self.subscriptions.write().clear();
    }
}

impl Default for NodeCache {
    fn default() -> Self {
        Self::new()
    }
}

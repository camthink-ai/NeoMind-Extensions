// metrics.rs
use neomind_extension_sdk::{
    ExtensionMetricValue, MetricDataType, MetricDescriptor, MetricValue,
    dynamic_metrics::{DynamicMetricsRegistry, MetricTemplate},
};
use crate::db::Zone;

const BASE_EQUIP: &str = "gym.equipment_occupied";

pub struct Metrics {
    reg: DynamicMetricsRegistry,
}

impl Metrics {
    pub fn new() -> Self {
        // 1=occupied / 0=idle gauge. base_name carries the `gym.` prefix so the
        // derived per-instance name (`gym.equipment_occupied.<zone>`) groups with
        // the static aggregates `gym.present_count` / `gym.visits_today`.
        let templates = vec![
            MetricTemplate::new(BASE_EQUIP, "Equipment Occupied · {}", MetricDataType::Integer)
                .with_min(0.0)
                .with_max(1.0),
        ];
        Self { reg: DynamicMetricsRegistry::new(templates) }
    }

    /// Sync registry to the current zone set. Call after any zone change.
    ///
    /// Full-resync: clear + re-upsert enabled zones. Acceptable for small zone
    /// counts (typical gym has <50 zones). `instance_id` = zone uuid (stable
    /// internal key, survives reload/rename); `label` = zone name (readable,
    /// dashboard-facing; UNIQUE per Task 4 so labels can't collide).
    pub fn sync_zones(&self, zones: &[Zone]) {
        self.reg.clear();
        for z in zones {
            if z.enabled {
                self.reg.upsert(&z.id, &z.name);
            }
        }
    }

    pub fn set_occupied(&self, zone_id: &str, occupied: bool) {
        self.reg.set(
            zone_id,
            BASE_EQUIP,
            MetricValue::Integer(if occupied { 1 } else { 0 }),
        );
    }

    /// Static aggregate descriptors advertised via Extension::metrics().
    pub fn static_descriptors() -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor::new(
                "gym.present_count",
                "Gym · Present Count",
                MetricDataType::Integer,
            ),
            MetricDescriptor::new(
                "gym.visits_today",
                "Gym · Visits Today",
                MetricDataType::Integer,
            ),
        ]
    }

    /// Produce all values: static aggregates + per-zone (registry).
    ///
    /// `present_count` / `visits_today` are computed by the caller (Task 10 /
    /// lib.rs) — this module does no I/O or session computation. `visits_today`
    /// is 0 in P1 (sessions land in P2).
    ///
    /// `timestamp` is **milliseconds since Unix epoch** (matches the SDK's
    /// `ExtensionMetricValue.timestamp` unit). All emitted values — static and
    /// per-zone — share this same timestamp via `with_timestamp` /
    /// `reg.values(ts)` so downstream charting sees a uniform unit.
    pub fn produce(
        &self,
        present_count: i64,
        visits_today: i64,
        timestamp: i64,
    ) -> Vec<ExtensionMetricValue> {
        let mut out = vec![
            ExtensionMetricValue::with_timestamp(
                "gym.present_count",
                MetricValue::Integer(present_count),
                timestamp,
            ),
            ExtensionMetricValue::with_timestamp(
                "gym.visits_today",
                MetricValue::Integer(visits_today),
                timestamp,
            ),
        ];
        out.extend(self.reg.values(timestamp));
        out
    }

    pub fn descriptors(&self) -> Vec<MetricDescriptor> {
        let mut out = Self::static_descriptors();
        out.extend(self.reg.descriptors());
        out
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(id: &str, name: &str, enabled: bool) -> Zone {
        Zone {
            id: id.into(),
            name: name.into(),
            equipment_type: "treadmill".into(),
            polygon: vec![],
            enabled,
        }
    }

    #[test]
    fn descriptors_grow_with_zones() {
        let m = Metrics::new();
        // Static aggregates present with no zones.
        assert!(m.descriptors().iter().any(|d| d.name == "gym.present_count"));
        assert!(m.descriptors().iter().any(|d| d.name == "gym.visits_today"));

        let z = zone("u1", "跑步机区", true);
        m.sync_zones(std::slice::from_ref(&z));
        let names: Vec<_> = m.descriptors().iter().map(|d| d.name.clone()).collect();
        assert!(
            names.iter().any(|n| n.starts_with("gym.equipment_occupied.")),
            "got {:?}",
            names
        );

        m.set_occupied("u1", true);
        let vals = m.produce(2, 5, 0);
        assert!(vals.iter().any(|v| v.name == "gym.present_count"));
        assert!(vals.iter().any(|v| v.name == "gym.visits_today"));
        // Dynamic per-zone value emitted with the occupied flag.
        assert!(
            vals.iter().any(|v| v.name.starts_with("gym.equipment_occupied.")),
            "got {:?}",
            vals.iter().map(|v| &v.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn disabled_zone_not_advertised() {
        let m = Metrics::new();
        let z = zone("u1", "拉伸区", false);
        m.sync_zones(&[z]);
        let descs = m.descriptors();
        assert!(
            !descs.iter().any(|d| d.name.contains("拉伸区")),
            "disabled zone leaked into descriptors: {:?}",
            descs.iter().map(|d| &d.name).collect::<Vec<_>>()
        );
        // values() also empty of dynamic entries.
        let vals = m.produce(0, 0, 0);
        assert!(vals.iter().all(|v| !v.name.starts_with("gym.equipment_occupied.")));
    }

    #[test]
    fn set_occupied_before_sync_is_noop() {
        // set on an unregistered instance_id silently no-ops (SDK contract).
        let m = Metrics::new();
        m.set_occupied("ghost", true);
        let vals = m.produce(0, 0, 0);
        assert!(vals.iter().all(|v| !v.name.starts_with("gym.equipment_occupied.")));
    }

    #[test]
    fn occupied_flag_round_trips() {
        let m = Metrics::new();
        // Hyphens are preserved by sanitize_label (only `.` and whitespace → `_`).
        m.sync_zones(&[zone("u1", "free-weights", true)]);
        m.set_occupied("u1", true);
        let v_true = m
            .produce(0, 0, 0)
            .into_iter()
            .find(|v| v.name == "gym.equipment_occupied.free-weights")
            .expect("occupied metric emitted");
        match v_true.value {
            MetricValue::Integer(n) => assert_eq!(n, 1),
            other => panic!("expected Integer, got {:?}", other),
        }
        m.set_occupied("u1", false);
        let v_false = m
            .produce(0, 0, 0)
            .into_iter()
            .find(|v| v.name == "gym.equipment_occupied.free-weights")
            .expect("idle metric emitted");
        match v_false.value {
            MetricValue::Integer(n) => assert_eq!(n, 0),
            other => panic!("expected Integer, got {:?}", other),
        }
    }

    #[test]
    fn sync_zones_is_idempotent_full_resync() {
        // Full-resync clears stale zones: a removed zone disappears from descriptors.
        let m = Metrics::new();
        m.sync_zones(&[
            zone("u1", "zone-a", true),
            zone("u2", "zone-b", true),
        ]);
        assert_eq!(
            m.descriptors()
                .iter()
                .filter(|d| d.name.starts_with("gym.equipment_occupied."))
                .count(),
            2
        );
        // Re-sync with only zone-a — zone-b must be gone.
        m.sync_zones(&[zone("u1", "zone-a", true)]);
        let dyn_names: Vec<_> = m
            .descriptors()
            .iter()
            .filter(|d| d.name.starts_with("gym.equipment_occupied."))
            .map(|d| d.name.clone())
            .collect::<Vec<_>>();
        assert_eq!(dyn_names, vec!["gym.equipment_occupied.zone-a"]);
    }
}

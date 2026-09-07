// metrics.rs
use crate::db::Zone;
use crate::geo;
use crate::types::Track;
use neomind_extension_sdk::{
    dynamic_metrics::{DynamicMetricsRegistry, MetricTemplate},
    ExtensionMetricValue, MetricDataType, MetricDescriptor, MetricValue,
};

const BASE_EQUIP: &str = "gym.equipment_occupied";

pub struct Metrics {
    reg: DynamicMetricsRegistry,
}

impl Metrics {
    pub fn new() -> Self {
        // 1=occupied / 0=idle gauge. base_name carries the `gym.` prefix so the
        // derived per-instance name (`gym.equipment_occupied.<zone>`) groups with
        // the static aggregates `gym.present_count` / `gym.visits_today`.
        let templates = vec![MetricTemplate::new(
            BASE_EQUIP,
            "Equipment Occupied · {}",
            MetricDataType::Integer,
        )
        .with_min(0.0)
        .with_max(1.0)];
        Self {
            reg: DynamicMetricsRegistry::new(templates),
        }
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

    /// Recompute per-zone occupation from the current live tracks + zone set.
    /// A zone is occupied iff at least one track's `foot` is inside its polygon
    /// (ray-cast). Disabled zones are skipped. Zones absent from the registry
    /// are silently ignored by `set_occupied` (SDK no-op contract).
    pub fn apply_occupation(&self, tracks: &[Track], zones: &[Zone]) {
        for z in zones {
            if !z.enabled {
                continue;
            }
            let occupied = tracks
                .iter()
                .any(|t| geo::point_in_polygon(t.foot.x, t.foot.y, &z.polygon));
            self.set_occupied(&z.id, occupied);
        }
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
    use crate::types::{Bbox, Point, Track};

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
        assert!(m
            .descriptors()
            .iter()
            .any(|d| d.name == "gym.present_count"));
        assert!(m.descriptors().iter().any(|d| d.name == "gym.visits_today"));

        let z = zone("u1", "跑步机区", true);
        m.sync_zones(std::slice::from_ref(&z));
        let names: Vec<_> = m.descriptors().iter().map(|d| d.name.clone()).collect();
        assert!(
            names
                .iter()
                .any(|n| n.starts_with("gym.equipment_occupied.")),
            "got {:?}",
            names
        );

        m.set_occupied("u1", true);
        let vals = m.produce(2, 5, 0);
        assert!(vals.iter().any(|v| v.name == "gym.present_count"));
        assert!(vals.iter().any(|v| v.name == "gym.visits_today"));
        // Dynamic per-zone value emitted with the occupied flag.
        assert!(
            vals.iter()
                .any(|v| v.name.starts_with("gym.equipment_occupied.")),
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
        assert!(vals
            .iter()
            .all(|v| !v.name.starts_with("gym.equipment_occupied.")));
    }

    #[test]
    fn set_occupied_before_sync_is_noop() {
        // set on an unregistered instance_id silently no-ops (SDK contract).
        let m = Metrics::new();
        m.set_occupied("ghost", true);
        let vals = m.produce(0, 0, 0);
        assert!(vals
            .iter()
            .all(|v| !v.name.starts_with("gym.equipment_occupied.")));
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
        m.sync_zones(&[zone("u1", "zone-a", true), zone("u2", "zone-b", true)]);
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

    fn zone_poly(id: &str, name: &str, polygon: Vec<(f32, f32)>) -> Zone {
        Zone {
            id: id.into(),
            name: name.into(),
            equipment_type: "treadmill".into(),
            polygon,
            enabled: true,
        }
    }

    fn track_foot(tid: i64, x: f32, y: f32) -> Track {
        Track {
            track_id: tid,
            ts: None,
            bbox: Bbox {
                x,
                y,
                w: 0.0,
                h: 0.0,
            },
            foot: Point { x, y },
            pose: None,
            face: None,
            vel: None,
        }
    }

    /// Read the Integer value of a named produced metric (panics if absent or
    /// non-integer) so the occupation assertions stay one-liners.
    fn int_val(vals: &[ExtensionMetricValue], name: &str) -> i64 {
        let v = vals
            .iter()
            .find(|v| v.name == name)
            .unwrap_or_else(|| panic!("metric {name} not in produced set"));
        match &v.value {
            MetricValue::Integer(n) => *n,
            other => panic!("expected Integer for {name}, got {other:?}"),
        }
    }

    #[test]
    fn apply_occupation_marks_only_zones_with_a_foot_inside() {
        let m = Metrics::new();
        let z1 = zone_poly(
            "z1",
            "Treadmill",
            vec![(0.2, 0.2), (0.8, 0.2), (0.8, 0.8), (0.2, 0.8)],
        );
        let z2 = zone_poly(
            "z2",
            "Bench",
            vec![(0.0, 0.0), (0.1, 0.0), (0.1, 0.1), (0.0, 0.1)],
        );
        m.sync_zones(&[z1.clone(), z2.clone()]);

        // Track A's foot is inside z1; track B's foot is inside neither zone.
        let tracks = vec![track_foot(1, 0.5, 0.5), track_foot(2, 0.95, 0.95)];
        m.apply_occupation(&tracks, &[z1, z2]);

        let vals = m.produce(2, 0, 0);
        assert_eq!(
            int_val(&vals, "gym.equipment_occupied.Treadmill"),
            1,
            "foot inside z1"
        );
        assert_eq!(
            int_val(&vals, "gym.equipment_occupied.Bench"),
            0,
            "no foot inside z2"
        );
    }

    #[test]
    fn apply_occupation_goes_idle_when_tracks_leave() {
        let m = Metrics::new();
        let z1 = zone_poly(
            "z1",
            "Treadmill",
            vec![(0.2, 0.2), (0.8, 0.2), (0.8, 0.8), (0.2, 0.8)],
        );
        m.sync_zones(std::slice::from_ref(&z1));

        // Occupied while a foot is inside...
        m.apply_occupation(&[track_foot(1, 0.5, 0.5)], std::slice::from_ref(&z1));
        assert_eq!(
            int_val(&m.produce(1, 0, 0), "gym.equipment_occupied.Treadmill"),
            1
        );

        // ...then the person leaves (no tracks) → must flip back to idle.
        m.apply_occupation(&[], &[z1]);
        assert_eq!(
            int_val(&m.produce(0, 0, 0), "gym.equipment_occupied.Treadmill"),
            0
        );
    }

    #[test]
    fn apply_occupation_skips_disabled_zones() {
        let m = Metrics::new();
        let mut z1 = zone_poly(
            "z1",
            "Treadmill",
            vec![(0.2, 0.2), (0.8, 0.2), (0.8, 0.8), (0.2, 0.8)],
        );
        z1.enabled = false;
        // sync_zones already skips disabled zones, so z1 is never registered;
        // apply_occupation must skip it too (no phantom metric, no panic).
        m.sync_zones(std::slice::from_ref(&z1));
        m.apply_occupation(&[track_foot(1, 0.5, 0.5)], &[z1]);
        let vals = m.produce(0, 0, 0);
        assert!(
            vals.iter()
                .all(|v| !v.name.starts_with("gym.equipment_occupied.")),
            "disabled zone must produce no per-zone metric"
        );
    }
}

# Dynamic Metrics Guide

How to expose **per-instance** time-series metrics from a NeoMind extension.

## When to use dynamic metrics

Use dynamic metrics when your extension tracks **multiple parallel instances**
at runtime and each instance needs:

- (a) **independent time-series queries** (e.g. chart one stream's fps without
  filtering JSON blobs), and/or
- (b) **independent alert thresholds** (e.g. alert when `fps.cam1 < 10` but
  tolerate `fps.cam2 < 5`), and/or
- (c) **its own dashboard card configuration**.

| Domain | "Instance" | Typical metrics |
|--------|-----------|-----------------|
| Video stream processing | each active stream | `fps`, `dropped_frames`, `inference_ms` |
| Batch / job processing | each job | duration, items processed, error rate |
| Voice assistant | each session | first-token latency, token count, barge-ins |
| Parallel inference | each worker | queue depth, throughput |

If your "instance" is fundamentally a **physical device** that should appear in
NeoMind's device list / device-level alerts, use the existing **device model**
(`homeassistant-bridge`, `lorawan-bridge`, `modbus-bridge`) instead — that path
gives you full device lifecycle, areas, and rules integration for free.

If you only have one logical thing (no parallel instances), use **static
metrics** — return a fixed `Vec<MetricDescriptor>` from `metrics()`. Dynamic
metrics add bookkeeping that single-instance extensions don't need.

## The three patterns at a glance

| Pattern | When | Storage shape |
|---------|------|---------------|
| **Static metrics** | One instance, fixed schema | `MetricDescriptor` list declared at compile time |
| **Dynamic metrics** (this guide) | N parallel instances, same schema per instance | `<base_metric>.<label>` time series, discovered at runtime |
| **Device model** | Physical devices needing full NeoMind device integration | Per-device metrics under `device_id`, with areas/rules |

## The naming contract

```
metric_name := <base_metric>.<label>
```

- `base_metric` — semantic metric name (`fps`, `latency_ms`, `job_duration`)
- `label` — stable, human-readable identifier for the instance (`cam1`,
  `task-abc123`, `session-42`)

The host does not interpret labels; **the extension is fully responsible for
generating and stabilizing them**.

## Using `DynamicMetricsRegistry`

The SDK ships a reusable helper at
`neomind_extension_sdk::DynamicMetricsRegistry`. It is optional — you can
inline the same pattern yourself — but it removes the boilerplate of tracking
instances + expanding templates.

```rust
use neomind_extension_sdk::{
    DynamicMetricsRegistry, MetricTemplate, MetricDataType, MetricValue,
};

pub struct MyExtension {
    dynamic: DynamicMetricsRegistry,
}

impl MyExtension {
    pub fn new() -> Self {
        let dynamic = DynamicMetricsRegistry::new(vec![
            MetricTemplate::new("fps", "FPS · {}", MetricDataType::Float)
                .with_unit("fps")
                .with_min(0.0),
            MetricTemplate::new("dropped_frames", "Dropped · {}", MetricDataType::Integer)
                .with_unit("frames")
                .with_min(0.0),
        ]);
        Self { dynamic }
    }
}
```

Lifecycle hooks (instance starts / stops):

```rust
// Stream / job / session starts
self.dynamic.upsert(&session_id, &label);
self.dynamic.set(&session_id, "fps", MetricValue::Float(29.97));

// ... instance runs; keep calling `set` as new values arrive ...

// Stream / job / session stops — must remove or series lingers in discovery
self.dynamic.remove(&session_id);
```

Wire into the trait:

```rust
fn metrics(&self) -> Vec<MetricDescriptor> {
    let mut d = vec![
        // Extension-level aggregates stay static
        MetricDescriptor::new("active_streams", "Active Streams", MetricDataType::Integer),
    ];
    d.extend(self.dynamic.descriptors());
    d
}

fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut out = vec![
        ExtensionMetricValue::new("active_streams", MetricValue::Integer(active_count)),
    ];
    out.extend(self.dynamic.values(now));
    Ok(out)
}
```

## Label design guide

A good label is:

1. **Stable** — the same logical instance always produces the same label across
   reconnects / restarts. This keeps the time series contiguous.
2. **Readable** — a user looking at the dashboard should recognize which
   instance `fps.cam1` refers to.
3. **Deduplicated** — if the same business entity can be instantiated multiple
   ways (e.g. RTSP reconnect), derive the label from the stable identifier
   (e.g. the URL path tail), not from a transient session UUID.
4. **Short** — keep ≤ 32 characters to keep dashboard UI readable.

Characters `.` and whitespace in labels are auto-replaced with `_` by the SDK
helper, so the `<base>.<label>` boundary stays unambiguous. Other punctuation
passes through unchanged.

Example derivation for video streams (`yolo-video-v2`):

```rust
fn derive_label(source_url: &str) -> String {
    // "rtsp://192.168.1.10/cam1" → "cam1"
    // "camera://0"                → "0"
    let after_scheme = source_url.split_once("://").map(|(_, r)| r).unwrap_or(source_url);
    after_scheme.split('?').next().unwrap_or(after_scheme)
        .trim_end_matches('/')
        .rsplit('/').next().unwrap_or(after_scheme)
        .to_string()
}
```

## Discovery latency

The host refreshes each extension's descriptor on a TTL (default **60 s**),
aligned with the metric-poll cycle. After an instance is created or removed:

- `produce_metrics()` reflects the new state on the **next poll** (≤ 60 s)
- `metrics()` (what `/api/extensions` advertises) reflects it within one
  additional TTL window

For typical dashboards this latency is fine. If you need faster discovery,
lower the TTL via `ExtensionMetricsCollector::with_descriptor_ttl(...)`.

## Anti-patterns

1. **Using a raw UUID as the label.**
   Users can't tell `fps.8a3f2b…` from `fps.7c1d9e…`. Always derive a
   human-readable identifier.

2. **Regenerating labels every frame / every second.**
   This explodes the series count. The label must be stable for the lifetime
   of a logical instance.

3. **Forgetting `remove()` when an instance ends.**
   The host's discovery list won't prune the orphan descriptor until the
   process restarts, and historical data sits in storage forever. Always pair
   `upsert` with a corresponding `remove` in your close/stop path.

4. **Stuffing every derived value into a dynamic metric.**
   If a value doesn't need (a) independent queries, (b) independent alerts, or
   (c) its own card, leave it in a JSON aggregate field (`streams_status`,
   `detected_classes`). Reserve dynamic metrics for things that genuinely need
   their own series.

5. **Reusing a label across different physical instances.**
   The host has no series-deletion API, so historical samples for a label
   persist indefinitely. If `cam1` is reassigned to a different physical
   camera tomorrow, the new samples append to the old `fps.cam1` series —
   charts will show a discontinuity rather than a fresh start. Make labels
   unique to the underlying entity (include device serial / stream URL tail),
   not just to a logical slot that can be repopulated.

## Limitations

- **Series cleanup is manual.** When an instance is removed, the descriptor
  disappears from `/api/extensions` within one TTL window, but historical
  samples remain in storage. A future host-side series-deletion API will
  address this; for now, prefer stable, business-unique labels (see
  anti-pattern 5) so colliding data is at least chartable on the same axis.
- **Discovery latency is bounded by the TTL** (default 60 s). New instances
  appear in `/api/extensions` within one TTL window after `upsert`. Lower
  the TTL via `ExtensionMetricsCollector::with_descriptor_ttl(...)` if you
  need faster discovery.

## Reference implementation

`extensions/yolo-video-v2/src/lib.rs` is the canonical example:

- 4 templates (`fps`, `dropped_frames`, `frame_count`, `detection_count`)
- `upsert` on `init_session` and `recover_session`
- `set` inside the existing `produce_metrics` aggregation loop
- `remove` on `close_session`
- Static extension-level aggregates (`active_streams`, `total_frames_processed`)
  preserved alongside for backwards compatibility

Future candidates: `image-analyzer-v2` (per-job metrics), voice-assistant
(per-session first-token latency).

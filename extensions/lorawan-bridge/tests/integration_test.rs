//! Integration tests for lorawan-bridge extension.
//!
//! Uses a minimal MQTT broker implemented with tokio to test MQTT uplink handling.
//! All tests are #[ignore] gated — run with:
//!
//!     cargo test -p lorawan-bridge -- --ignored

use std::time::Duration;

use neomind_extension_lorawan_bridge::LorawanBridgeExtension;
use neomind_extension_sdk::Extension;
use base64::Engine;

// ---------------------------------------------------------------------------
// Minimal MQTT broker
// ---------------------------------------------------------------------------

/// A minimal MQTT v3.1.1 broker that handles CONNECT/SUBSCRIBE/PUBLISH.
/// Unlike rumqttd, this broker handles client disconnect gracefully.
mod mini_broker {
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    /// A simple topic matcher: checks if a subscription pattern matches a publish topic.
    /// Supports `+` (single-level) and `#` (multi-level) wildcards.
    pub fn topic_matches(pattern: &str, topic: &str) -> bool {
        let pattern_parts: Vec<&str> = pattern.split('/').collect();
        let topic_parts: Vec<&str> = topic.split('/').collect();

        let mut pi = 0;
        let mut ti = 0;

        while pi < pattern_parts.len() && ti < topic_parts.len() {
            match pattern_parts[pi] {
                "#" => return true,
                "+" => {
                    pi += 1;
                    ti += 1;
                }
                p => {
                    if p != topic_parts[ti] {
                        return false;
                    }
                    pi += 1;
                    ti += 1;
                }
            }
        }

        if pi < pattern_parts.len() && pattern_parts[pi] == "#" {
            return true;
        }

        pi == pattern_parts.len() && ti == topic_parts.len()
    }

    struct Subscription {
        pattern: String,
        tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    }

    pub struct MiniBroker {
        subscriptions: Arc<Mutex<Vec<Subscription>>>,
    }

    impl MiniBroker {
        pub fn new() -> Self {
            Self {
                subscriptions: Arc::new(Mutex::new(Vec::new())),
            }
        }

        pub async fn start(&self, port: u16) {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port))
                .await
                .expect("Failed to bind broker");

            let subs = self.subscriptions.clone();

            tokio::spawn(async move {
                loop {
                    let (stream, _) = match listener.accept().await {
                        Ok(s) => s,
                        Err(_) => continue,
                    };

                    let subs = subs.clone();

                    tokio::spawn(async move {
                        let _ = handle_client(stream, subs).await;
                    });
                }
            });
        }
    }

    /// Parse MQTT variable-length encoding for remaining length.
    /// Returns (remaining_length, bytes_consumed) or None on error.
    fn parse_remaining_length(buf: &[u8]) -> Option<(usize, usize)> {
        let mut multiplier = 1;
        let mut value = 0;
        let mut idx = 1; // Start after the packet type byte
        loop {
            if idx >= buf.len() {
                return None;
            }
            let byte = buf[idx];
            value += ((byte & 0x7F) as usize) * multiplier;
            idx += 1;
            if (byte & 0x80) == 0 {
                break;
            }
            multiplier *= 128;
            if multiplier > 128 * 128 * 128 {
                return None; // Malformed
            }
        }
        Some((value, idx))
    }

    async fn handle_client(
        stream: TcpStream,
        subs: Arc<Mutex<Vec<Subscription>>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (read_half, write_half) = tokio::io::split(stream);
        let mut reader = tokio::io::BufReader::new(read_half);
        let writer = Arc::new(tokio::sync::Mutex::new(write_half));
        let mut buf = vec![0u8; 8192];

        // Read CONNECT packet
        let n = reader.read(&mut buf).await?;
        if n < 14 || buf[0] != 0x10 {
            return Err("Expected CONNECT packet".into());
        }

        // Send CONNACK
        let connack = [0x20, 0x02, 0x00, 0x00];
        let mut w = writer.lock().await;
        w.write_all(&connack).await?;
        drop(w);

        // Channel for forwarding published messages to this client
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);

        // Read loop for incoming packets
        let subs_clone = subs.clone();
        let writer_clone = writer.clone();
        let read_task: tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>> =
            tokio::spawn(async move {
                loop {
                    let n = match reader.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };

                    if n < 2 {
                        continue;
                    }

                    let packet_type = buf[0] >> 4;

                    match packet_type {
                        // SUBSCRIBE (8)
                        8 => {
                            let (_remaining_len, header_len) = match parse_remaining_length(&buf[..n]) {
                                Some(v) => v,
                                None => continue,
                            };
                            let payload_start = header_len;
                            if payload_start + 4 > n {
                                continue;
                            }
                            let _packet_id = ((buf[payload_start] as u16) << 8)
                                | buf[payload_start + 1] as u16;

                            let topic_len = ((buf[payload_start + 2] as usize) << 8)
                                | buf[payload_start + 3] as usize;
                            if payload_start + 4 + topic_len > n {
                                continue;
                            }
                            let topic_filter = String::from_utf8_lossy(
                                &buf[payload_start + 4..payload_start + 4 + topic_len],
                            )
                            .to_string();

                            {
                                let mut s = subs_clone.lock().unwrap();
                                s.push(Subscription {
                                    pattern: topic_filter,
                                    tx: tx.clone(),
                                });
                            }

                            let suback = [
                                0x90,
                                0x03,
                                buf[payload_start],
                                buf[payload_start + 1],
                                0x00,
                            ];
                            let mut w = writer_clone.lock().await;
                            let _ = w.write_all(&suback).await;
                        }
                        // PUBLISH (3)
                        3 => {
                            let (_remaining_len, header_len) = match parse_remaining_length(&buf[..n]) {
                                Some(v) => v,
                                None => continue,
                            };
                            let topic_start = header_len;
                            if topic_start + 2 > n {
                                continue;
                            }
                            let topic_len = ((buf[topic_start] as usize) << 8)
                                | buf[topic_start + 1] as usize;
                            if topic_start + 2 + topic_len > n {
                                continue;
                            }
                            let topic = String::from_utf8_lossy(
                                &buf[topic_start + 2..topic_start + 2 + topic_len],
                            )
                            .to_string();

                            let subs = subs_clone.lock().unwrap();
                            for sub in subs.iter() {
                                if topic_matches(&sub.pattern, &topic) {
                                    let _ = sub.tx.try_send(buf[..n].to_vec());
                                }
                            }
                        }
                        // DISCONNECT (14)
                        14 => break,
                        // PINGREQ (12)
                        12 => {
                            let pingresp = [0xD0, 0x00];
                            let mut w = writer_clone.lock().await;
                            let _ = w.write_all(&pingresp).await;
                        }
                        _ => {}
                    }
                }
                Ok(())
            });

        // Write loop — forward messages from other clients
        let write_task: tokio::task::JoinHandle<()> = tokio::spawn(async move {
            while let Some(data) = rx.recv().await {
                let mut w = writer.lock().await;
                if w.write_all(&data).await.is_err() {
                    break;
                }
            }
        });

        let _ = read_task.await;
        let _ = write_task.await;

        Ok(())
    }
}

// Re-export for use in tests
use mini_broker::MiniBroker;

/// Start a minimal MQTT broker on a random port.
fn start_mqtt_broker() -> u16 {
    // Find a random available port
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let port = addr.port();
    drop(listener);

    // Create a separate tokio runtime for the broker on a dedicated thread.
    // We can't use the test runtime because the broker needs to outlive
    // individual tests and handle concurrent connections.
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();

        let broker = MiniBroker::new();
        rt.block_on(async {
            broker.start(port).await;
            // Keep the runtime alive
            std::future::pending::<()>().await;
        });
    });

    // Give broker time to start
    std::thread::sleep(Duration::from_millis(300));
    port
}

/// Publish a message using rumqttc.
async fn publish_message(broker_port: u16, topic: &str, payload: &str) {
    use rumqttc::{AsyncClient, MqttOptions, QoS};

    let mut options = MqttOptions::new(
        format!("test-pub-{}", std::process::id()),
        "127.0.0.1",
        broker_port,
    );
    options.set_keep_alive(Duration::from_secs(10));

    let (client, mut eventloop) = AsyncClient::new(options, 10);

    // Wait for connection and drain initial events
    for _ in 0..30 {
        match eventloop.poll().await {
            Ok(rumqttc::Event::Incoming(rumqttc::Incoming::ConnAck(_))) => break,
            Ok(_) => continue,
            Err(e) => {
                eprintln!("[test-publisher] Connection error: {}", e);
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        }
    }

    client
        .publish(topic, QoS::AtLeastOnce, false, payload.as_bytes())
        .await
        .unwrap();

    // Drive the event loop to actually send the PUBLISH packet
    for _ in 0..10 {
        let _ = eventloop.poll().await;
    }

    // Wait for broker to process
    tokio::time::sleep(Duration::from_millis(500)).await;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_connect_to_chirpstack() {
    let port = start_mqtt_broker();

    let ext = LorawanBridgeExtension::new();

    let result = ext
        .execute_command(
            "connect",
            &serde_json::json!({
                "ns_type": "chirpstack",
                "broker_url": format!("tcp://127.0.0.1:{}", port),
                "application_id": "1",
                "auto_discover": true
            }),
        )
        .await
        .unwrap();

    assert_eq!(result["success"], true);
    assert!(result["message"].as_str().unwrap().contains("Chirpstack"));

    // Check status
    let status = ext.execute_command("get_status", &serde_json::json!({})).await.unwrap();
    assert_eq!(status["connected"], true);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_chirpstack_uplink_device_discovery() {
    let port = start_mqtt_broker();

    let ext = LorawanBridgeExtension::new();

    let _ = ext
        .execute_command(
            "connect",
            &serde_json::json!({
                "ns_type": "chirpstack",
                "broker_url": format!("tcp://127.0.0.1:{}", port),
                "application_id": "1",
                "auto_discover": true
            }),
        )
        .await
        .unwrap();

    // Wait for subscription to be established
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Publish a ChirpStack uplink message
    let topic = "application/1/device/0102030405060708/event/up";
    let payload = serde_json::json!({
        "devEui": "0102030405060708",
        "fCnt": 42,
        "object": {
            "temperature": 23.5,
            "humidity": 65
        },
        "rxInfo": [{"rssi": -57, "snr": 8.2}],
        "fPort": 2
    })
    .to_string();

    publish_message(port, topic, &payload).await;

    // Wait for message processing
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Verify device was discovered
    let list = ext.execute_command("list_devices", &serde_json::json!({})).await.unwrap();
    assert_eq!(list["count"], 1);
    assert_eq!(list["devices"][0]["dev_eui"], "0102030405060708");
    assert_eq!(list["devices"][0]["rssi"], -57);

    // Verify decoded fields
    let device = ext
        .execute_command(
            "get_device",
            &serde_json::json!({"dev_eui": "0102030405060708"}),
        )
        .await
        .unwrap();
    assert_eq!(device["device"]["fields"].as_array().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_ttn_uplink_device_discovery() {
    let port = start_mqtt_broker();

    let ext = LorawanBridgeExtension::new();

    let _ = ext
        .execute_command(
            "connect",
            &serde_json::json!({
                "ns_type": "ttn",
                "broker_url": format!("tcp://127.0.0.1:{}", port),
                "application_id": "test-app",
                "tenant_id": "ttn",
                "auto_discover": true
            }),
        )
        .await
        .unwrap();

    // Wait for subscription
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Publish a TTN v3 uplink message
    let topic = "v3/test-app@ttn/devices/my-device-1/up";
    let payload = serde_json::json!({
        "end_device_ids": {
            "device_id": "my-device-1",
            "application_ids": {"application_id": "test-app"}
        },
        "uplink_message": {
            "f_cnt": 42,
            "decoded_payload": {
                "temperature": 23.5,
                "humidity": 60
            },
            "rx_metadata": [{"rssi": -70, "snr": 5.5}],
            "f_port": 2
        }
    })
    .to_string();

    publish_message(port, topic, &payload).await;

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Verify device was discovered
    let list = ext.execute_command("list_devices", &serde_json::json!({})).await.unwrap();
    assert_eq!(list["count"], 1);
    assert_eq!(list["devices"][0]["dev_eui"], "my-device-1");
    assert_eq!(list["devices"][0]["f_cnt"], 42);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cayenne_lpp_decoding() {
    let port = start_mqtt_broker();

    let ext = LorawanBridgeExtension::new();

    let _ = ext
        .execute_command(
            "connect",
            &serde_json::json!({
                "ns_type": "chirpstack",
                "broker_url": format!("tcp://127.0.0.1:{}", port),
                "application_id": "1",
                "auto_discover": true
            }),
        )
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Cayenne LPP: channel 0, type 0x67 (temperature), value 0x0064 = 10.0°C
    let lpp_bytes: Vec<u8> = vec![0x00, 0x67, 0x00, 0x64];
    let data_b64 = base64::engine::general_purpose::STANDARD.encode(&lpp_bytes);

    let topic = "application/1/device/ABCDEF1234567890/event/up";
    let payload = serde_json::json!({
        "devEui": "ABCDEF1234567890",
        "fCnt": 7,
        "data": data_b64,
        "rxInfo": [{"rssi": -60, "snr": 7.5}],
        "fPort": 2
    })
    .to_string();

    publish_message(port, topic, &payload).await;

    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Verify Cayenne-decoded values
    let device = ext
        .execute_command(
            "get_device",
            &serde_json::json!({"dev_eui": "ABCDEF1234567890"}),
        )
        .await
        .unwrap();

    let fields = device["device"]["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0]["name"], "temperature");
    let temp_val = fields[0]["value"].as_f64().unwrap();
    assert!((temp_val - 10.0).abs() < 0.1, "Expected 10.0, got {}", temp_val);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_produce_metrics_with_devices() {
    let port = start_mqtt_broker();

    let ext = LorawanBridgeExtension::new();

    let _ = ext
        .execute_command(
            "connect",
            &serde_json::json!({
                "ns_type": "chirpstack",
                "broker_url": format!("tcp://127.0.0.1:{}", port),
                "application_id": "1",
                "auto_discover": true
            }),
        )
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Publish a message
    let topic = "application/1/device/DEADBEEF01020304/event/up";
    let payload = serde_json::json!({
        "devEui": "DEADBEEF01020304",
        "fCnt": 1,
        "object": {"temperature": 25.0},
        "rxInfo": [{"rssi": -50, "snr": 9.0}],
        "fPort": 1
    })
    .to_string();

    publish_message(port, topic, &payload).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let metrics = ext.produce_metrics().unwrap();

    // Should have: total_commands, connected, device_count, plus per-device metrics
    assert!(metrics.len() >= 5, "Expected at least 5 metrics, got {}", metrics.len());

    let device_count = metrics.iter().find(|m| m.name == "device_count").unwrap();
    match &device_count.value {
        neomind_extension_sdk::ParamMetricValue::Integer(v) => assert_eq!(*v, 1),
        _ => panic!("Expected Integer"),
    }

    // Check for per-device metrics
    let temp_metric = metrics.iter().find(|m| m.name == "lorawan.DEADBEEF01020304.temperature");
    assert!(temp_metric.is_some(), "Should have temperature metric");
}

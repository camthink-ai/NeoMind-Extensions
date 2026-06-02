use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::apdu;
use crate::types::*;

/// Manages a single BACnet device's state and polling
pub struct BacnetDeviceManager {
    pub device_id: u32,
    pub ip_address: String,
    pub port: u16,
    pub poll_interval_ms: u64,
    running: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl BacnetDeviceManager {
    pub fn new(
        device_id: u32,
        ip_address: String,
        port: u16,
        poll_interval_ms: u64,
    ) -> Self {
        Self {
            device_id,
            ip_address,
            port,
            poll_interval_ms,
            running: Arc::new(AtomicBool::new(false)),
            handle: None,
        }
    }

    /// Start background polling thread
    pub fn start(
        &mut self,
        devices: Arc<parking_lot::RwLock<std::collections::HashMap<u32, BacnetDevice>>>,
        bind_address: String,
        _bind_port: u16,
        timeout_ms: u64,
    ) -> Result<(), String> {
        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        let device_id = self.device_id;
        let ip = self.ip_address.clone();
        let port = self.port;
        let interval = self.poll_interval_ms;

        let handle = std::thread::Builder::new()
            .name(format!("bacnet-poll-{}", device_id))
            .spawn(move || {
                // Create a socket for polling — use an ephemeral port
                let socket = match std::net::UdpSocket::bind(format!("{}:0", bind_address)) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!(
                            "[bacnet-bridge] Poll thread socket bind failed for device {}: {}",
                            device_id, e
                        );
                        return;
                    }
                };

                socket
                    .set_read_timeout(Some(std::time::Duration::from_millis(timeout_ms)))
                    .ok();

                socket.set_broadcast(true).ok();

                let target = format!("{}:{}", ip, port);
                let mut consecutive_failures: u32 = 0;
                const MAX_FAILURES_BEFORE_DISCONNECT: u32 = 3;

                while running.load(Ordering::SeqCst) {
                    let objects: Vec<BacnetObject> = {
                        let devices_r = devices.read();
                        match devices_r.get(&device_id) {
                            Some(d) => d
                                .objects
                                .iter()
                                .filter(|o| o.object_type != BacnetObjectType::Device)
                                .cloned()
                                .collect(),
                            None => break,
                        }
                    };

                    if objects.is_empty() {
                        // No objects to poll — sleep and retry
                        let sleep_duration = std::time::Duration::from_millis(interval);
                        let sleep_start = std::time::Instant::now();
                        while running.load(Ordering::SeqCst)
                            && sleep_start.elapsed() < sleep_duration
                        {
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                        continue;
                    }

                    // Poll each object's present value
                    for obj in &objects {
                        let msg = apdu::build_read_property(
                            device_id,
                            obj.object_type,
                            obj.instance,
                            apdu::PROPERTY_PRESENT_VALUE,
                        );

                        if let Ok((response, _)) =
                            send_and_receive(&socket, &target, &msg)
                        {
                            if let Some(apdu::ApduResponse::ReadPropertyAck {
                                value,
                                ..
                            }) = apdu::parse_response(&response)
                            {
                                consecutive_failures = 0;
                                let mut devices_w = devices.write();
                                if let Some(device) = devices_w.get_mut(&device_id) {
                                    for o in &mut device.objects {
                                        if o.object_type == obj.object_type
                                            && o.instance == obj.instance
                                        {
                                            o.present_value = Some(value.clone());
                                        }
                                    }
                                    device.connected = true;
                                    device.last_seen_ms =
                                        chrono::Utc::now().timestamp_millis();
                                }
                            }
                        } else {
                            consecutive_failures += 1;
                            if consecutive_failures >= MAX_FAILURES_BEFORE_DISCONNECT {
                                let mut devices_w = devices.write();
                                if let Some(device) = devices_w.get_mut(&device_id) {
                                    device.connected = false;
                                }
                            }
                        }

                        // Small delay between polls to avoid overwhelming the device
                        if !running.load(Ordering::SeqCst) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }

                    // Sleep for remaining interval
                    let sleep_start = std::time::Instant::now();
                    while running.load(Ordering::SeqCst)
                        && sleep_start.elapsed().as_millis() < interval as u128
                    {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }

                eprintln!("[bacnet-bridge] Poll thread stopped for device {}", device_id);
            })
            .map_err(|e| format!("Failed to spawn polling thread: {}", e))?;

        self.handle = Some(handle);
        Ok(())
    }

    /// Stop polling
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    /// Update poll interval (takes effect on next poll cycle)
    pub fn update_poll_interval(&mut self, interval_ms: u64) {
        self.poll_interval_ms = interval_ms;
    }
}

impl Drop for BacnetDeviceManager {
    fn drop(&mut self) {
        self.stop();
    }
}

fn send_and_receive(
    socket: &std::net::UdpSocket,
    target: &str,
    message: &[u8],
) -> Result<(Vec<u8>, String), String> {
    socket
        .send_to(message, target)
        .map_err(|e| format!("Send failed: {}", e))?;

    let mut buf = [0u8; 2048];
    match socket.recv_from(&mut buf) {
        Ok((len, addr)) => Ok((buf[..len].to_vec(), addr.to_string())),
        Err(e) => Err(format!("Receive failed: {}", e)),
    }
}

use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::apdu;
use crate::types::*;

/// BACnet/IP client for sending requests and receiving responses
pub struct BacnetClient {
    socket: UdpSocket,
    timeout_ms: u64,
}

impl BacnetClient {
    /// Create a new BACnet client bound to the specified address.
    /// Uses OS-assigned port (port 0) to avoid conflicts with the listener socket.
    pub fn new(bind_address: &str, _bind_port: u16, timeout_ms: u64) -> Result<Self, String> {
        // Use port 0 to let the OS assign a random available port.
        // This avoids conflicts with the listener socket and between sequential commands.
        let addr = format!("{}:0", bind_address);
        let socket = UdpSocket::bind(&addr)
            .map_err(|e| format!("Failed to bind BACnet socket to {}: {}", addr, e))?;

        socket
            .set_broadcast(true)
            .map_err(|e| format!("Failed to enable broadcast: {}", e))?;

        socket
            .set_read_timeout(Some(Duration::from_millis(timeout_ms)))
            .map_err(|e| format!("Failed to set read timeout: {}", e))?;

        Ok(Self {
            socket,
            timeout_ms,
        })
    }

    /// Send a Who-Is broadcast to discover devices
    pub fn send_who_is(&self, low: u32, high: u32) -> Result<(), String> {
        let msg = apdu::build_who_is(low, high);

        self.socket
            .send_to(&msg, "255.255.255.255:47808")
            .map_err(|e| format!("Failed to send Who-Is: {}", e))?;

        Ok(())
    }

    /// Send a message to a specific device and wait for response
    pub fn send_and_receive(
        &self,
        target_addr: &str,
        message: &[u8],
    ) -> Result<(Vec<u8>, String), String> {
        self.socket
            .send_to(message, target_addr)
            .map_err(|e| format!("Failed to send to {}: {}", target_addr, e))?;

        let mut buf = [0u8; 2048];
        match self.socket.recv_from(&mut buf) {
            Ok((len, addr)) => Ok((buf[..len].to_vec(), addr.to_string())),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Send a message and collect multiple responses (for Who-Is I-Am)
    pub fn send_and_collect_responses(
        &self,
        message: &[u8],
        target_addr: &str,
        wait_ms: u64,
    ) -> Vec<(Vec<u8>, String)> {
        if let Err(e) = self.socket.send_to(message, target_addr) {
            eprintln!("[bacnet-bridge] Send failed: {}", e);
            return Vec::new();
        }

        let mut responses = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_millis(wait_ms);

        // Temporarily shorten timeout for collection loop
        let _ = self
            .socket
            .set_read_timeout(Some(Duration::from_millis(500)));

        while std::time::Instant::now() < deadline {
            let mut buf = [0u8; 2048];
            match self.socket.recv_from(&mut buf) {
                Ok((len, addr)) => {
                    responses.push((buf[..len].to_vec(), addr.to_string()));
                }
                Err(_) => {
                    // Timeout on this recv, continue collecting
                }
            }
        }

        // Restore original timeout
        let _ = self
            .socket
            .set_read_timeout(Some(Duration::from_millis(self.timeout_ms)));

        responses
    }

    /// Receive a single message (for background listener)
    pub fn receive(&self) -> Result<(Vec<u8>, String), String> {
        let mut buf = [0u8; 2048];
        match self.socket.recv_from(&mut buf) {
            Ok((len, addr)) => Ok((buf[..len].to_vec(), addr.to_string())),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                {
                    Err("timeout".to_string())
                } else {
                    Err(format!("Receive error: {}", e))
                }
            }
        }
    }

    /// Get the local address this client is bound to
    pub fn local_addr(&self) -> Result<String, String> {
        self.socket
            .local_addr()
            .map(|a| a.to_string())
            .map_err(|e| format!("Failed to get local addr: {}", e))
    }

    pub fn set_timeout(&self, timeout_ms: u64) -> Result<(), String> {
        self.socket
            .set_read_timeout(Some(Duration::from_millis(timeout_ms)))
            .map_err(|e| format!("Failed to set timeout: {}", e))
    }
}

/// Background listener for I-Am responses and COV notifications
pub fn start_listener(
    bind_address: String,
    bind_port: u16,
    devices: Arc<parking_lot::RwLock<std::collections::HashMap<u32, BacnetDevice>>>,
    cov_subscriptions: Arc<parking_lot::RwLock<std::collections::HashMap<u32, CovSubscription>>>,
    running: Arc<AtomicBool>,
) -> Result<std::thread::JoinHandle<()>, String> {
    // Use a different port for the listener to avoid conflicts with the command socket
    let socket = UdpSocket::bind(format!("{}:{}", bind_address, bind_port))
        .map_err(|e| format!("Listener bind failed on port {}: {}", bind_port, e))?;

    socket
        .set_read_timeout(Some(Duration::from_millis(1000)))
        .map_err(|e| format!("Listener timeout set failed: {}", e))?;

    socket
        .set_broadcast(true)
        .map_err(|e| format!("Listener broadcast enable failed: {}", e))?;

    let handle = std::thread::Builder::new()
        .name("bacnet-listener".to_string())
        .spawn(move || {
            let mut buf = [0u8; 2048];

            while running.load(Ordering::SeqCst) {
                match socket.recv_from(&mut buf) {
                    Ok((len, addr)) => {
                        let data = &buf[..len];
                        let addr_str = addr.to_string();

                        if let Some(response) = apdu::parse_response(data) {
                            match response {
                                apdu::ApduResponse::IAm {
                                    device_id,
                                    max_apdu,
                                    segmentation,
                                    vendor_id,
                                } => {
                                    let mut devices = devices.write();
                                    let ip_port = parse_ip_port(&addr_str);

                                    if let Some(device) = devices.get_mut(&device_id) {
                                        device.connected = true;
                                        device.last_seen_ms =
                                            chrono::Utc::now().timestamp_millis();
                                        device.max_apdu = Some(max_apdu);
                                        device.vendor_id = Some(vendor_id);
                                        device.ip_address = ip_port.0;
                                        device.port = ip_port.1;
                                    } else {
                                        // New device discovered
                                        devices.insert(
                                            device_id,
                                            BacnetDevice {
                                                device_id,
                                                ip_address: ip_port.0,
                                                port: ip_port.1,
                                                name: None,
                                                vendor_id: Some(vendor_id),
                                                vendor_name: None,
                                                model: None,
                                                firmware: None,
                                                description: None,
                                                max_apdu: Some(max_apdu),
                                                segmentation: Some(format!("{}", segmentation)),
                                                objects: Vec::new(),
                                                connected: true,
                                                last_seen_ms: chrono::Utc::now()
                                                    .timestamp_millis(),
                                            },
                                        );
                                    }
                                }
                                apdu::ApduResponse::CovNotification {
                                    subscriber_id,
                                    device_id,
                                    object_type,
                                    instance,
                                    values,
                                } => {
                                    // Update COV subscription last update
                                    let mut covs = cov_subscriptions.write();
                                    if let Some(sub) = covs.get_mut(&subscriber_id) {
                                        sub.last_update_ms =
                                            chrono::Utc::now().timestamp_millis();
                                        sub.active = true;
                                    }
                                    drop(covs);

                                    // Update cached values
                                    let mut devices = devices.write();
                                    if let Some(device) = devices.get_mut(&device_id) {
                                        for obj in &mut device.objects {
                                            if obj.object_type == object_type
                                                && obj.instance == instance
                                            {
                                                for (prop_id, value) in &values {
                                                    if *prop_id == apdu::PROPERTY_PRESENT_VALUE {
                                                        obj.present_value = Some(value.clone());
                                                    }
                                                }
                                                obj.cov_subscribed = true;
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(_) => {} // Timeout, continue
                }
            }
        })
        .map_err(|e| format!("Failed to spawn listener thread: {}", e))?;

    Ok(handle)
}

pub fn parse_ip_port(addr: &str) -> (String, u16) {
    let parts: Vec<&str> = addr.rsplitn(2, ':').collect();
    if parts.len() == 2 {
        let port = parts[0].parse().unwrap_or(47808);
        let ip = parts[1].trim_start_matches('[').trim_end_matches(']');
        (ip.to_string(), port)
    } else {
        (addr.to_string(), 47808)
    }
}

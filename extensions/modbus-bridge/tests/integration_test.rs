//! Integration tests for modbus-bridge extension.
//!
//! These tests embed a Modbus TCP server directly and test the extension
//! commands against it. All tests are #[ignore] gated — run with:
//!
//!     cargo test -p modbus-bridge -- --ignored
//!
//! # Architecture Note
//!
//! The modbus-bridge extension uses `tokio_modbus::sync` client internally,
//! which creates its own `new_current_thread` tokio runtime per connection.
//! This means we CANNOT call `execute_command("read_registers", ...)` etc.
//! from within an existing tokio runtime — the nested runtime creation panics.
//!
//! The polling loop works fine because it runs on its own bare OS thread.
//! So we test read/write through the polling path (produce_metrics) rather
//! than through direct execute_command calls.

use std::collections::HashMap;
use std::future;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use neomind_extension_modbus_bridge::ModbusBridgeExtension;
use neomind_extension_sdk::Extension;
use tokio_modbus::{prelude::*, server::Service};
use tokio_modbus::server::tcp::{accept_tcp_connection, Server};

// ---------------------------------------------------------------------------
// Embedded Modbus TCP server
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct ModbusService {
    registers: Arc<Mutex<HashMap<u16, u16>>>,
    coils: Arc<Mutex<HashMap<u16, bool>>>,
}

impl Service for ModbusService {
    type Request = Request<'static>;
    type Response = Response;
    type Exception = ExceptionCode;
    type Future = future::Ready<Result<Response, ExceptionCode>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        let res = match req {
            Request::ReadHoldingRegisters(addr, cnt) => {
                let regs = self.registers.lock().unwrap();
                let values: Vec<u16> = (addr..addr + cnt)
                    .map(|a| regs.get(&a).copied().unwrap_or(0))
                    .collect();
                Ok(Response::ReadHoldingRegisters(values))
            }
            Request::WriteSingleRegister(addr, val) => {
                self.registers.lock().unwrap().insert(addr, val);
                Ok(Response::WriteSingleRegister(addr, val))
            }
            Request::WriteMultipleRegisters(addr, values) => {
                let mut regs = self.registers.lock().unwrap();
                for (i, &v) in values.iter().enumerate() {
                    regs.insert(addr + i as u16, v);
                }
                Ok(Response::WriteMultipleRegisters(addr, values.len() as u16))
            }
            Request::ReadCoils(addr, cnt) => {
                let coils = self.coils.lock().unwrap();
                let values: Vec<bool> = (addr..addr + cnt)
                    .map(|a| coils.get(&a).copied().unwrap_or(false))
                    .collect();
                Ok(Response::ReadCoils(values))
            }
            Request::WriteSingleCoil(addr, val) => {
                self.coils.lock().unwrap().insert(addr, val);
                Ok(Response::WriteSingleCoil(addr, val))
            }
            Request::WriteMultipleCoils(addr, values) => {
                let mut coils = self.coils.lock().unwrap();
                for (i, &v) in values.iter().enumerate() {
                    coils.insert(addr + i as u16, v);
                }
                Ok(Response::WriteMultipleCoils(addr, values.len() as u16))
            }
            _ => Err(ExceptionCode::IllegalFunction),
        };
        future::ready(res)
    }
}

/// Start an embedded Modbus TCP server on a random port.
/// Returns (actual_addr, registers, coils) so tests can inspect server state.
async fn start_modbus_server() -> (SocketAddr, Arc<Mutex<HashMap<u16, u16>>>, Arc<Mutex<HashMap<u16, bool>>>) {
    let registers = Arc::new(Mutex::new(HashMap::new()));
    let coils = Arc::new(Mutex::new(HashMap::new()));

    // Pre-populate test data matching the Python simulator:
    // Reg 0-1: temperature = 23.5°C (float32)
    let temp_bits = 23.5f32.to_bits();
    let temp_hi = (temp_bits >> 16) as u16;
    let temp_lo = temp_bits as u16;
    {
        let mut r = registers.lock().unwrap();
        r.insert(0, temp_hi);
        r.insert(1, temp_lo);
        r.insert(2, 65);    // humidity = 65%
        r.insert(3, 2201);  // voltage = 2201 (×0.1 = 220.1V)
    }
    {
        let mut c = coils.lock().unwrap();
        c.insert(0, true);  // status ON
        c.insert(1, false); // alarm OFF
    }

    let regs_clone = registers.clone();
    let coils_clone = coils.clone();

    // Bind to port 0 to get a random available port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let server = Server::new(listener);
        let on_connected = |stream, socket_addr| {
            let regs = regs_clone.clone();
            let coils = coils_clone.clone();
            async move {
                let new_service = |_addr: SocketAddr| {
                    Ok(Some(ModbusService {
                        registers: regs.clone(),
                        coils: coils.clone(),
                    }))
                };
                accept_tcp_connection(stream, socket_addr, new_service)
            }
        };
        let on_error = |err| eprintln!("Server error: {err}");
        let _ = server.serve(&on_connected, on_error).await;
    });

    // Give the server a moment to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    (addr, registers, coils)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_add_device_and_connect() {
    let (addr, _regs, _coils) = start_modbus_server().await;
    let port = addr.port();

    let ext = std::sync::Arc::new(ModbusBridgeExtension::new());

    let result = ext.execute_command("add_device", &serde_json::json!({
        "device": {
            "device_id": "test-plc",
            "name": "Test PLC",
            "mode": "tcp",
            "ip": "127.0.0.1",
            "port": port,
            "slave_id": 1,
            "poll_interval_ms": 60000,
            "registers": [
                { "address": 0, "count": 2, "name": "temperature", "type": "float32", "unit": "°C" },
            ]
        }
    })).await.unwrap();

    assert_eq!(result["success"], true);
    assert_eq!(result["device_id"], "test-plc");

    // Give the polling thread time to connect
    tokio::time::sleep(Duration::from_millis(500)).await;

    let list = ext.execute_command("list_devices", &serde_json::json!({})).await.unwrap();
    assert_eq!(list["count"], 1);
    assert_eq!(list["devices"][0]["device_id"], "test-plc");

    // Cleanup
    let _ = ext.execute_command("remove_device", &serde_json::json!({"device_id": "test-plc"})).await;
}

/// Test that the background polling loop correctly reads float32 and uint16 values
/// from the Modbus server and exposes them via produce_metrics.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_polling_reads_float32_and_uint16() {
    let (addr, _regs, _coils) = start_modbus_server().await;
    let port = addr.port();

    let ext = std::sync::Arc::new(ModbusBridgeExtension::new());

    let _ = ext.execute_command("add_device", &serde_json::json!({
        "device": {
            "device_id": "test-poll",
            "mode": "tcp",
            "ip": "127.0.0.1",
            "port": port,
            "slave_id": 1,
            "poll_interval_ms": 200,
            "registers": [
                { "address": 0, "count": 2, "name": "temperature", "type": "float32", "unit": "°C" },
                { "address": 2, "count": 1, "name": "humidity", "type": "uint16", "unit": "%" },
                { "address": 3, "count": 1, "name": "voltage", "type": "uint16", "unit": "V", "scale": 0.1 }
            ]
        }
    })).await.unwrap();

    // Wait for at least one poll cycle to complete
    tokio::time::sleep(Duration::from_millis(800)).await;

    // Check metrics — the polling loop should have read and decoded the registers
    let metrics = ext.produce_metrics().unwrap();

    // Find the temperature metric (float32 = 23.5°C)
    let temp_metric = metrics.iter().find(|m| m.name == "modbus.test-poll.temperature");
    assert!(temp_metric.is_some(), "Should have temperature metric");
    if let Some(m) = temp_metric {
        match &m.value {
            neomind_extension_sdk::ParamMetricValue::Float(v) => {
                assert!((v - 23.5).abs() < 0.1, "Expected 23.5, got {}", v);
            }
            _ => panic!("Expected Float for temperature"),
        }
    }

    // Find the humidity metric (uint16 = 65%)
    let hum_metric = metrics.iter().find(|m| m.name == "modbus.test-poll.humidity");
    assert!(hum_metric.is_some(), "Should have humidity metric");
    if let Some(m) = hum_metric {
        match &m.value {
            neomind_extension_sdk::ParamMetricValue::Float(v) => {
                assert!((*v as u16) == 65, "Expected 65, got {}", v);
            }
            other => panic!("Expected numeric for humidity, got {:?}", other),
        }
    }

    // Find the voltage metric (uint16 = 2201, scaled by 0.1 = 220.1V)
    let volt_metric = metrics.iter().find(|m| m.name == "modbus.test-poll.voltage");
    assert!(volt_metric.is_some(), "Should have voltage metric");

    // Cleanup
    let _ = ext.execute_command("remove_device", &serde_json::json!({"device_id": "test-poll"})).await;
}

/// Test write_register via polling verification:
/// Add a device, poll to verify initial state, then use a child process
/// to test write (since write_register also uses the sync client).
/// Instead, we verify that the polling loop correctly detects connection state.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_device_connection_status() {
    let (addr, _regs, _coils) = start_modbus_server().await;
    let port = addr.port();

    let ext = std::sync::Arc::new(ModbusBridgeExtension::new());

    let _ = ext.execute_command("add_device", &serde_json::json!({
        "device": {
            "device_id": "test-conn",
            "mode": "tcp",
            "ip": "127.0.0.1",
            "port": port,
            "slave_id": 1,
            "poll_interval_ms": 200,
            "registers": []
        }
    })).await.unwrap();

    // Wait for polling to establish connection
    tokio::time::sleep(Duration::from_millis(500)).await;

    let metrics = ext.produce_metrics().unwrap();

    // Check that connected_devices metric reflects our device
    let connected = metrics.iter().find(|m| m.name == "connected_devices");
    assert!(connected.is_some(), "Should have connected_devices metric");

    // Check per-device connected metric
    let device_connected = metrics.iter().find(|m| m.name == "modbus.test-conn.connected");
    assert!(device_connected.is_some(), "Should have per-device connected metric");

    // Cleanup
    let _ = ext.execute_command("remove_device", &serde_json::json!({"device_id": "test-conn"})).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_remove_device() {
    let (addr, _regs, _coils) = start_modbus_server().await;
    let port = addr.port();

    let ext = std::sync::Arc::new(ModbusBridgeExtension::new());

    let _ = ext.execute_command("add_device", &serde_json::json!({
        "device": {
            "device_id": "test-remove",
            "mode": "tcp",
            "ip": "127.0.0.1",
            "port": port,
            "slave_id": 1,
            "poll_interval_ms": 60000,
            "registers": []
        }
    })).await.unwrap();

    let list = ext.execute_command("list_devices", &serde_json::json!({})).await.unwrap();
    assert_eq!(list["count"], 1);

    // Remove the device
    let result = ext.execute_command("remove_device", &serde_json::json!({
        "device_id": "test-remove"
    })).await.unwrap();
    assert_eq!(result["success"], true);

    // Verify it's gone
    let list = ext.execute_command("list_devices", &serde_json::json!({})).await.unwrap();
    assert_eq!(list["count"], 0);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_produce_metrics() {
    let (addr, _regs, _coils) = start_modbus_server().await;
    let port = addr.port();

    let ext = std::sync::Arc::new(ModbusBridgeExtension::new());

    let _ = ext.execute_command("add_device", &serde_json::json!({
        "device": {
            "device_id": "test-metrics",
            "mode": "tcp",
            "ip": "127.0.0.1",
            "port": port,
            "slave_id": 1,
            "poll_interval_ms": 500,
            "registers": [
                { "address": 0, "count": 2, "name": "temperature", "type": "float32", "unit": "°C" },
                { "address": 2, "count": 1, "name": "humidity", "type": "uint16", "unit": "%" }
            ]
        }
    })).await.unwrap();

    // Wait for at least one poll cycle
    tokio::time::sleep(Duration::from_millis(1000)).await;

    let metrics = ext.produce_metrics().unwrap();

    // Should have at least: total_commands, connected_devices, total_poll_errors,
    // plus per-device metrics (temperature, humidity, connected, poll_errors, last_poll_ms)
    assert!(metrics.len() >= 8, "Expected at least 8 metrics, got {}", metrics.len());

    // Check total_commands metric
    let total_cmds = metrics.iter().find(|m| m.name == "total_commands").unwrap();
    match &total_cmds.value {
        neomind_extension_sdk::ParamMetricValue::Integer(v) => assert!(*v >= 1),
        _ => panic!("Expected Integer for total_commands"),
    }

    // Check per-device temperature metric
    let temp_metric = metrics.iter().find(|m| m.name == "modbus.test-metrics.temperature");
    assert!(temp_metric.is_some(), "Should have temperature metric");

    // Cleanup
    let _ = ext.execute_command("remove_device", &serde_json::json!({"device_id": "test-metrics"})).await;
}

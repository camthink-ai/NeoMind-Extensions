use crate::register_map::decode_register;
use crate::types::{ConnectionMode, DeviceConfig, DeviceState, RegisterValue};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_modbus::prelude::*;

pub struct ModbusDevice {
    config: DeviceConfig,
    state: Arc<Mutex<DeviceState>>,
    running: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl ModbusDevice {
    pub fn new(config: DeviceConfig) -> Self {
        let state = Arc::new(Mutex::new(DeviceState {
            register_values: vec![],
            connected: false,
            poll_errors: 0,
            last_poll_ms: 0,
            config: config.clone(),
        }));
        Self {
            config,
            state,
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(AtomicOrdering::SeqCst) {
            return Err("Already running".into());
        }
        self.running.store(true, AtomicOrdering::SeqCst);

        let config = self.config.clone();
        let state = self.state.clone();
        let running = self.running.clone();

        let handle = std::thread::Builder::new()
            .name(format!("modbus-{}", config.device_id))
            .spawn(move || {
                polling_loop(config, state, running);
            })
            .map_err(|e| format!("Thread spawn error: {}", e))?;

        self.thread_handle = Some(handle);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.running.store(false, AtomicOrdering::SeqCst);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }

    pub fn get_state(&self) -> DeviceState {
        self.state.lock().unwrap().clone()
    }

    pub fn update_poll_interval(&mut self, interval_ms: u64) {
        self.config.poll_interval_ms = interval_ms;
        if let Ok(mut s) = self.state.lock() {
            s.config.poll_interval_ms = interval_ms;
        }
    }

    pub fn read_registers(&self, start: u16, count: u16) -> Result<Vec<u16>, String> {
        let mut ctx = connect_sync(&self.config)?;
        ctx.read_holding_registers(start, count)
            .map_err(|e| format!("Read error: {}", e))?
            .map_err(|e| format!("Read exception: {:?}", e))
    }

    pub fn write_register(&self, address: u16, value: u16) -> Result<(), String> {
        let mut ctx = connect_sync(&self.config)?;
        ctx.write_single_register(address, value)
            .map_err(|e| format!("Write error: {}", e))?
            .map_err(|e| format!("Write exception: {:?}", e))
    }

    pub fn write_coil(&self, address: u16, value: bool) -> Result<(), String> {
        let mut ctx = connect_sync(&self.config)?;
        ctx.write_single_coil(address, value)
            .map_err(|e| format!("Write coil error: {}", e))?
            .map_err(|e| format!("Write coil exception: {:?}", e))
    }
}

impl Drop for ModbusDevice {
    fn drop(&mut self) {
        self.stop();
    }
}

fn connect_sync(config: &DeviceConfig) -> Result<sync::Context, String> {
    match config.mode {
        ConnectionMode::Tcp => {
            let ip = config.ip.as_deref().ok_or("Missing IP address")?;
            let port = config.port.unwrap_or(502);
            let socket_addr = format!("{}:{}", ip, port)
                .parse()
                .map_err(|e| format!("Invalid address: {}", e))?;
            sync::tcp::connect_slave(socket_addr, Slave(config.slave_id))
                .map_err(|e| format!("TCP connect error: {}", e))
        }
        ConnectionMode::Rtu => {
            let serial = config.serial_port.as_deref().ok_or("Missing serial port")?;
            let baud = config.baud_rate.unwrap_or(9600);
            let builder = tokio_serial::new(serial, baud);
            sync::rtu::connect_slave(&builder, Slave(config.slave_id))
                .map_err(|e| format!("RTU connect error: {}", e))
        }
    }
}

fn polling_loop(
    config: DeviceConfig,
    state: Arc<Mutex<DeviceState>>,
    running: Arc<AtomicBool>,
) {
    while running.load(AtomicOrdering::SeqCst) {
        let interval = {
            let s = state.lock().unwrap();
            s.config.poll_interval_ms
        };

        let start = Instant::now();

        match poll_all_registers(&config) {
            Ok(values) => {
                let elapsed = start.elapsed().as_millis() as u64;
                let mut s = state.lock().unwrap();
                s.register_values = values;
                s.connected = true;
                s.poll_errors = 0;
                s.last_poll_ms = elapsed;
            }
            Err(_) => {
                let mut s = state.lock().unwrap();
                s.connected = false;
                s.poll_errors = s.poll_errors.saturating_add(1);
            }
        }

        let elapsed = start.elapsed().as_millis() as u64;
        let sleep_ms = if interval > elapsed {
            interval - elapsed
        } else {
            100
        };
        if running.load(AtomicOrdering::SeqCst) {
            std::thread::sleep(Duration::from_millis(sleep_ms));
        }
    }
}

fn poll_all_registers(config: &DeviceConfig) -> Result<Vec<RegisterValue>, String> {
    let mut ctx = connect_sync(config)?;
    let mut values = Vec::new();

    for reg in &config.registers {
        let words = ctx
            .read_holding_registers(reg.address, reg.count)
            .map_err(|e| format!("Read register {} error: {}", reg.name, e))?
            .map_err(|e| format!("Read register {} exception: {:?}", reg.name, e))?;
        values.push(decode_register(reg, &words));
    }

    Ok(values)
}

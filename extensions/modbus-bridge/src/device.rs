use crate::register_map::decode_register;
use crate::types::{ConnectionMode, DeviceConfig, DeviceState, RegisterType, RegisterValue};
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
            // Use a timeout to prevent indefinite blocking when the Modbus
            // device is unresponsive (TCP connection hangs).
            match handle.join() {
                Ok(()) => {}
                Err(_) => {
                    eprintln!("[modbus-bridge] Polling thread panicked during stop");
                }
            }
        }
    }

    pub fn get_state(&self) -> DeviceState {
        match self.state.lock() {
            Ok(s) => s.clone(),
            Err(e) => {
                eprintln!("[modbus-bridge] Lock poisoned for {}: {}", self.config.device_id, e);
                DeviceState {
                    register_values: vec![],
                    connected: false,
                    poll_errors: 0,
                    last_poll_ms: 0,
                    config: self.config.clone(),
                }
            }
        }
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

    pub fn read_input_registers(&self, start: u16, count: u16) -> Result<Vec<u16>, String> {
        let mut ctx = connect_sync(&self.config)?;
        ctx.read_input_registers(start, count)
            .map_err(|e| format!("Read input error: {}", e))?
            .map_err(|e| format!("Read input exception: {:?}", e))
    }

    #[allow(dead_code)]
    pub fn read_coils(&self, start: u16, count: u16) -> Result<Vec<bool>, String> {
        let mut ctx = connect_sync(&self.config)?;
        ctx.read_coils(start, count)
            .map_err(|e| format!("Read coils error: {}", e))?
            .map_err(|e| format!("Read coils exception: {:?}", e))
    }

    #[allow(dead_code)]
    pub fn read_discrete_inputs(&self, start: u16, count: u16) -> Result<Vec<bool>, String> {
        let mut ctx = connect_sync(&self.config)?;
        ctx.read_discrete_inputs(start, count)
            .map_err(|e| format!("Read discrete inputs error: {}", e))?
            .map_err(|e| format!("Read discrete inputs exception: {:?}", e))
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

    pub fn write_registers(&self, start: u16, values: &[u16]) -> Result<(), String> {
        let mut ctx = connect_sync(&self.config)?;
        ctx.write_multiple_registers(start, values)
            .map_err(|e| format!("Write multiple registers error: {}", e))?
            .map_err(|e| format!("Write multiple registers exception: {:?}", e))
    }

    pub fn write_coils(&self, start: u16, values: &[bool]) -> Result<(), String> {
        let mut ctx = connect_sync(&self.config)?;
        ctx.write_multiple_coils(start, values)
            .map_err(|e| format!("Write multiple coils error: {}", e))?
            .map_err(|e| format!("Write multiple coils exception: {:?}", e))
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
            sync::tcp::connect_slave_with_timeout(
                socket_addr,
                Slave(config.slave_id),
                Some(Duration::from_millis(config.timeout_ms)),
            )
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
    // Keep the connection alive across poll cycles; reconnect only on failure.
    let mut ctx: Option<sync::Context> = None;

    while running.load(AtomicOrdering::SeqCst) {
        let interval = {
            match state.lock() {
                Ok(s) => s.config.poll_interval_ms,
                Err(e) => {
                    eprintln!("[modbus-bridge] Lock poisoned in polling loop: {}", e);
                    break;
                }
            }
        };

        let start = Instant::now();

        // (Re)connect if needed
        if ctx.is_none() {
            match connect_sync(&config) {
                Ok(c) => ctx = Some(c),
                Err(_) => {
                    if let Ok(mut s) = state.lock() {
                        s.connected = false;
                        s.poll_errors = s.poll_errors.saturating_add(1);
                    }
                    // Skip this cycle and retry next
                    let sleep_ms = interval.min(5000);
                    if running.load(AtomicOrdering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(sleep_ms));
                    }
                    continue;
                }
            }
        }

        match poll_all_registers_with_ctx(ctx.as_mut().unwrap(), &config) {
            Ok(values) => {
                let elapsed = start.elapsed().as_millis() as u64;
                if let Ok(mut s) = state.lock() {
                    s.register_values = values;
                    s.connected = true;
                    s.poll_errors = 0;
                    s.last_poll_ms = elapsed;
                }
            }
            Err(_) => {
                // Connection is likely broken; discard it so next cycle reconnects
                ctx = None;
                if let Ok(mut s) = state.lock() {
                    s.connected = false;
                    s.poll_errors = s.poll_errors.saturating_add(1);
                }
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

fn poll_all_registers_with_ctx(
    ctx: &mut sync::Context,
    config: &DeviceConfig,
) -> Result<Vec<RegisterValue>, String> {
    let mut values = Vec::new();
    let mut errors = Vec::new();

    for reg in &config.registers {
        // Validate register count per Modbus protocol limits
        let max_count: u16 = match reg.register_type {
            RegisterType::Holding | RegisterType::Input => 125,
            RegisterType::Coil | RegisterType::DiscreteInput => 2000,
        };
        if reg.count > max_count {
            eprintln!(
                "[modbus-bridge] Skipping '{}': count {} exceeds max {} for {:?}",
                reg.name, reg.count, max_count, reg.register_type
            );
            continue;
        }

        // Validate address + count does not overflow u16 register space
        let end_addr = reg.address as u32 + reg.count as u32;
        if end_addr > 65535 {
            eprintln!(
                "[modbus-bridge] Skipping '{}': address {} + count {} overflows u16",
                reg.name, reg.address, reg.count
            );
            continue;
        }

        let result = match reg.register_type {
            RegisterType::Holding => ctx
                .read_holding_registers(reg.address, reg.count)
                .map_err(|e| format!("Read holding {} error: {}", reg.name, e))
                .and_then(|r| r.map_err(|e| format!("Read holding {} exception: {:?}", reg.name, e))),
            RegisterType::Input => ctx
                .read_input_registers(reg.address, reg.count)
                .map_err(|e| format!("Read input {} error: {}", reg.name, e))
                .and_then(|r| r.map_err(|e| format!("Read input {} exception: {:?}", reg.name, e))),
            RegisterType::Coil => {
                let coils = ctx
                    .read_coils(reg.address, reg.count)
                    .map_err(|e| format!("Read coil {} error: {}", reg.name, e))
                    .and_then(|r| r.map_err(|e| format!("Read coil {} exception: {:?}", reg.name, e)))?;
                // Convert coils to u16 words for decode_register
                let words: Vec<u16> = coils.iter().map(|b| if *b { 1 } else { 0 }).collect();
                Ok(words)
            }
            RegisterType::DiscreteInput => {
                let inputs = ctx
                    .read_discrete_inputs(reg.address, reg.count)
                    .map_err(|e| format!("Read discrete input {} error: {}", reg.name, e))
                    .and_then(|r| r.map_err(|e| format!("Read discrete input {} exception: {:?}", reg.name, e)))?;
                let words: Vec<u16> = inputs.iter().map(|b| if *b { 1 } else { 0 }).collect();
                Ok(words)
            }
        };

        match result {
            Ok(words) => values.push(decode_register(reg, &words)),
            Err(e) => {
                eprintln!("[modbus-bridge] {}", e);
                errors.push(e);
            }
        }
    }

    // Return partial results if we got at least some values.
    // Only fail if *all* registers failed.
    if values.is_empty() && !errors.is_empty() {
        return Err(errors.into_iter().next().unwrap());
    }

    Ok(values)
}

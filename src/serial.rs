use crate::message::Message;
use crate::converter;
use log::info;
use serialport::{SerialPort, SerialPortInfo};
use std::io::{BufReader, Write};
use std::time::Duration;
use std::sync::mpsc;

/// Serial port configuration
pub struct SerialConfig {
    pub port: String,
    pub baud_rate: u32,
    pub timeout: Duration,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: "/dev/ttyUSB0".to_string(),
            baud_rate: 115200,
            timeout: Duration::from_secs(5),
        }
    }
}

/// Serial port handler
pub struct SerialHandler {
    port: Box<dyn SerialPort>,
    tx: mpsc::Sender<String>,
    rx: mpsc::Receiver<String>,
}

impl SerialHandler {
    /// Create a new SerialHandler with the given configuration
    pub fn new(config: SerialConfig) -> Result<Self, String> {
        info!("Opening serial port: {} at {} baud", config.port, config.baud_rate);
        
        let port = serialport::new(&config.port, config.baud_rate)
            .timeout(config.timeout)
            .open()
            .map_err(|e| format!("Failed to open serial port: {}", e))?;

        // Create channel for message passing
        let (tx, rx) = mpsc::channel::<String>();

        Ok(Self {
            port,
            tx,
            rx,
        })
    }

    /// Get a reader for the serial port (for non-blocking reads)
    pub fn reader(&mut self) -> BufReader<&mut dyn SerialPort> {
        BufReader::new(&mut *self.port)
    }

    /// Write a message to the serial port
    pub fn write_message(&mut self, msg: &Message) -> Result<(), String> {
        let serial_msg = converter::to_serial_message(msg);
        info!("Sending to hub: {}", serial_msg);

        self.port.write_all(serial_msg.as_bytes())
            .map_err(|e| format!("Failed to write to serial: {}", e))?;
        self.port.write_all(b"\n")
            .map_err(|e| format!("Failed to write newline: {}", e))?;
        self.port.flush()
            .map_err(|e| format!("Failed to flush serial: {}", e))?;

        Ok(())
    }

    /// Get receiver for messages from hub
    pub fn receiver(&self) -> &mpsc::Receiver<String> {
        &self.rx
    }

    /// Get sender for messages to hub
    pub fn sender(&self) -> &mpsc::Sender<String> {
        &self.tx
    }
}

/// List available serial ports
pub fn list_ports() -> Result<Vec<SerialPortInfo>, String> {
    serialport::available_ports()
        .map_err(|e| format!("Failed to list serial ports: {}", e))
}
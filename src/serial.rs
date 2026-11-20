//! Serial port communication with the hub (docs/v1/serial-communication-design.md).
//!
//! A dedicated OS thread owns the blocking `serialport` handle:
//! - it reads lines from the hub and forwards them as `SerialEvent::Line`,
//! - it drains `outgoing` (lines composed by the bridge) and writes them,
//! - when the port is lost it reopens with backoff and emits
//!   `SerialEvent::PortOpened` again so the host resends setup messages
//!   (covers "hub first time connect" and "host restarted", D6 in impl-plan.md),
//! - read/write errors are logged at most once per hour (legacy TODO.md item).

use std::io::{BufRead, BufReader};
use std::sync::mpsc::{channel as std_channel, Receiver as StdReceiver, Sender as StdSender};
use std::time::{Duration, Instant};

use log::{error, info, warn};
use serialport::{SerialPort, SerialPortInfo};
use tokio::sync::mpsc;

/// Events flowing from the serial thread to the async bridge.
#[derive(Debug)]
pub enum SerialEvent {
    /// One full line received from the hub.
    Line(String),
    /// The port was (re)opened; the host should resend setup messages.
    PortOpened,
}

/// Handles owned by the async side.
pub struct SerialChannel {
    /// Lines / events from the hub.
    pub incoming: mpsc::UnboundedReceiver<SerialEvent>,
    /// Send a line to the hub.
    pub outgoing: StdSender<String>,
}

/// Open the port (fails fast on startup) and spawn the reader/writer thread.
pub fn spawn(port_name: &str, baud_rate: u32) -> Result<SerialChannel, String> {
    // Validate the port opens before handing out the channel.
    let probe = open_port(port_name, baud_rate)?;
    drop(probe);

    let (incoming_tx, incoming) = mpsc::unbounded_channel::<SerialEvent>();
    let (outgoing_tx, outgoing_rx) = std_channel::<String>();

    let port_name = port_name.to_string();
    std::thread::Builder::new()
        .name("serial-hub".to_string())
        .spawn(move || run_loop(&port_name, baud_rate, incoming_tx, outgoing_rx))
        .map_err(|e| format!("failed to spawn serial thread: {}", e))?;

    Ok(SerialChannel { incoming, outgoing: outgoing_tx })
}

fn open_port(port_name: &str, baud_rate: u32) -> Result<Box<dyn SerialPort>, String> {
    serialport::new(port_name, baud_rate)
        .timeout(Duration::from_millis(100))
        .open()
        .map_err(|e| format!("failed to open serial port {}: {}", port_name, e))
}

fn run_loop(
    port_name: &str,
    baud_rate: u32,
    incoming_tx: mpsc::UnboundedSender<SerialEvent>,
    outgoing_rx: StdReceiver<String>,
) {
    let mut last_error_log = Instant::now() - Duration::from_secs(3600);

    loop {
        let mut port = match open_port(port_name, baud_rate) {
            Ok(p) => p,
            Err(e) => {
                rate_limited_error(&mut last_error_log, &e);
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        info!("Serial port {} opened at {} baud", port_name, baud_rate);

        let reader = match port.try_clone() {
            Ok(r) => BufReader::new(r),
            Err(e) => {
                rate_limited_error(&mut last_error_log, &format!("clone failed: {}", e));
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let mut reader = reader;

        // Notify the bridge so setup messages are (re)sent for this session.
        let _ = incoming_tx.send(SerialEvent::PortOpened);

        // One serial session: until the port errors out.
        // This loop already runs on the dedicated "serial-hub" OS thread
        // spawned in `spawn()`, so its blocking reads never stall the async
        // bridge or the API server. The 100ms read timeout plus the
        // non-blocking `try_recv` drain above keep the loop responsive to
        // both incoming lines and outgoing commands.
        'session: loop {
            // 1. drain outgoing lines (non-blocking)
            while let Ok(line) = outgoing_rx.try_recv() {
                if let Err(e) = write_line(&mut *port, &line) {
                    rate_limited_error(&mut last_error_log, &format!("serial write failed: {}", e));
                    break 'session;
                }
            }

            // 2. read one line (100ms timeout keeps the loop responsive)
            let mut buffer = String::new();
            match reader.read_line(&mut buffer) {
                Ok(0) => { /* no data in this window */ }
                Ok(_) => {
                    let line = buffer.trim();
                    if !line.is_empty() {
                        let _ = incoming_tx.send(SerialEvent::Line(line.to_string()));
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => {
                    rate_limited_error(
                        &mut last_error_log,
                        &format!("serial read failed (usb disconnected?): {}", e),
                    );
                    break 'session;
                }
            }
        }

        warn!("serial port lost; retrying in 1s");
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn write_line(port: &mut dyn SerialPort, line: &str) -> Result<(), String> {
    info!("Sending to hub: {}", line);
    port.write_all(line.as_bytes()).map_err(|e| format!("write: {}", e))?;
    port.write_all(b"\n").map_err(|e| format!("newline: {}", e))?;
    port.flush().map_err(|e| format!("flush: {}", e))?;
    Ok(())
}

/// Log at most once per hour (legacy TODO: "log error every hour when usb was disconnected").
fn rate_limited_error(last: &mut Instant, msg: &str) {
    if last.elapsed() >= Duration::from_secs(3600) {
        error!("{}", msg);
        *last = Instant::now();
    }
}

/// List available serial ports (used at startup for diagnostics).
pub fn list_ports() -> Result<Vec<SerialPortInfo>, String> {
    serialport::available_ports().map_err(|e| format!("failed to list serial ports: {}", e))
}

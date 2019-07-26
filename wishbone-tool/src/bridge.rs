use super::config::{Config, ConfigError};
use super::usb_bridge::UsbBridge;
use super::uart_bridge::UartBridge;
use std::sync::{Arc, Mutex};
use std::io;

pub enum BridgeServerKind {
    /// Wishbone bridge
    Wishbone,

    /// GDB server
    GDB,

    /// Send random data back and forth
    RandomTest,

    /// No server
    None,
}

pub enum BridgeKind {
    UsbBridge,
    UartBridge,
}

#[derive(Clone)]
pub enum Bridge {
    UsbBridge(UsbBridge),
    UartBridge(UartBridge),
}

#[derive(Debug)]
pub enum BridgeError {
    /// Expected one size, but got another
    LengthError(usize, usize),

    /// USB subsystem returned an error
    USBError(libusb::Error),

    /// std::io error
    IoError(io::Error),

    /// Attempted to communicate with the bridge, but it wasn't connected
    NotConnected,

    /// We got something weird back from the bridge
    WrongResponse,
}

impl std::convert::From<libusb::Error> for BridgeError {
    fn from(e: libusb::Error) -> BridgeError {
        BridgeError::USBError(e)
    }
}

impl std::convert::From<io::Error> for BridgeError {
    fn from(e: io::Error) -> BridgeError {
        BridgeError::IoError(e)
    }
}

impl BridgeServerKind {
    pub fn from_string(item: &Option<&str>) -> Result<BridgeServerKind, ConfigError> {
        match item {
            None => Ok(BridgeServerKind::None),
            Some(k) => match *k {
                "gdb" => Ok(BridgeServerKind::GDB),
                "wishbone" => Ok(BridgeServerKind::Wishbone),
                "random-test" => Ok(BridgeServerKind::RandomTest),
                unknown => Err(ConfigError::UnknownBridgeServerKind(unknown.to_owned())),
            },
        }
    }
}

impl Bridge {
    pub fn new(cfg: &Config) -> Result<Bridge, BridgeError> {
        match cfg.bridge_kind {
            BridgeKind::UartBridge => Ok(Bridge::UartBridge(UartBridge::new(cfg)?)),
            BridgeKind::UsbBridge => Ok(Bridge::UsbBridge(UsbBridge::new(cfg)?))
        }
    }

    pub fn connect(&self) -> Result<(), BridgeError> {
        match self {
            Bridge::UsbBridge(b) => b.connect(),
            Bridge::UartBridge(b) => b.connect(),
        }
    }

    pub fn mutex(&self) -> &Arc<Mutex<()>> {
        match self {
            Bridge::UsbBridge(b) => b.mutex(),
            Bridge::UartBridge(b) => b.mutex(),
        }
    }

    pub fn peek(&self, addr: u32) -> Result<u32, BridgeError> {
        let result = match self {
            Bridge::UsbBridge(b) => b.peek(addr),
            Bridge::UartBridge(b) => b.peek(addr),
        };
        result
    }

    pub fn poke(&self, addr: u32, value: u32) -> Result<(), BridgeError> {
        let result = match self {
            Bridge::UsbBridge(b) => b.poke(addr, value),
            Bridge::UartBridge(b) => b.poke(addr, value),
        };
        result
    }
}

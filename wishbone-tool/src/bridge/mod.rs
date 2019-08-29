pub mod uart;
pub mod usb;
pub mod spi;

use crate::config::Config;
use usb::UsbBridge;
use uart::UartBridge;
use spi::SpiBridge;
use std::sync::{Arc, Mutex};
use std::io;

pub enum BridgeKind {
    UsbBridge,
    UartBridge,
    SpiBridge,
}

#[derive(Clone)]
pub enum Bridge {
    UsbBridge(UsbBridge),
    UartBridge(UartBridge),
    SpiBridge(SpiBridge),
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

impl Bridge {
    pub fn new(cfg: &Config) -> Result<Bridge, BridgeError> {
        match cfg.bridge_kind {
            BridgeKind::UartBridge => Ok(Bridge::UartBridge(UartBridge::new(cfg)?)),
            BridgeKind::UsbBridge => Ok(Bridge::UsbBridge(UsbBridge::new(cfg)?)),
            BridgeKind::SpiBridge => Ok(Bridge::SpiBridge(SpiBridge::new(cfg)?)),
        }
    }

    pub fn connect(&self) -> Result<(), BridgeError> {
        match self {
            Bridge::UsbBridge(b) => b.connect(),
            Bridge::UartBridge(b) => b.connect(),
            Bridge::SpiBridge(b) => b.connect(),
        }
    }

    pub fn mutex(&self) -> &Arc<Mutex<()>> {
        match self {
            Bridge::UsbBridge(b) => b.mutex(),
            Bridge::UartBridge(b) => b.mutex(),
            Bridge::SpiBridge(b) => b.mutex(),
        }
    }

    pub fn peek(&self, addr: u32) -> Result<u32, BridgeError> {
        let result = match self {
            Bridge::UsbBridge(b) => b.peek(addr),
            Bridge::UartBridge(b) => b.peek(addr),
            Bridge::SpiBridge(b) => b.peek(addr),
        };
        result
    }

    pub fn poke(&self, addr: u32, value: u32) -> Result<(), BridgeError> {
        let result = match self {
            Bridge::UsbBridge(b) => b.poke(addr, value),
            Bridge::UartBridge(b) => b.poke(addr, value),
            Bridge::SpiBridge(b) => b.poke(addr, value),
        };
        result
    }
}

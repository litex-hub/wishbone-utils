use super::config::{Config, ConfigError};
use super::usb_bridge::UsbBridge;
use std::sync::{Arc, Mutex};

pub enum BridgeKind {
    /// Wishbone bridge
    Wishbone,

    /// GDB server
    GDB,

    /// Send random data back and forth
    RandomTest,

    /// No server
    None,
}

#[derive(Clone)]
pub enum Bridge {
    UsbBridge(UsbBridge),
}

#[derive(Debug, PartialEq)]
pub enum BridgeError {
    /// Expected one size, but got another
    LengthError(usize, usize),

    /// USB subsystem returned an error
    USBError(libusb::Error),

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

impl BridgeKind {
    pub fn from_string(item: &Option<&str>) -> Result<BridgeKind, ConfigError> {
        match item {
            None => Ok(BridgeKind::None),
            Some(k) => match *k {
                "gdb" => Ok(BridgeKind::GDB),
                "wishbone" => Ok(BridgeKind::Wishbone),
                "random-test" => Ok(BridgeKind::RandomTest),
                unknown => Err(ConfigError::UnknownBridgeKind(unknown.to_owned())),
            },
        }
    }
}

impl Bridge {
    pub fn new(cfg: &Config) -> Result<Bridge, BridgeError> {
        Ok(Bridge::UsbBridge(UsbBridge::new(cfg)?))
    }

    pub fn connect(&self) -> Result<(), BridgeError> {
        match self {
            Bridge::UsbBridge(b) => b.connect(),
        }
    }

    pub fn mutex(&self) -> &Arc<Mutex<()>> {
        match self {
            Bridge::UsbBridge(b) => b.mutex(),
        }
    }

    pub fn peek(&self, addr: u32) -> Result<u32, BridgeError> {
        let result = match self {
            Bridge::UsbBridge(b) => b.peek(addr),
        };
        // match result {
        //     Ok(v) => println!("<- R {:08x}: {:08x}", addr, v),
        //     Err(ref e) => println!("<- R {:08x}: {:?}", addr, e),
        // }
        result
    }

    pub fn poke(&self, addr: u32, value: u32) -> Result<(), BridgeError> {
        let result = match self {
            Bridge::UsbBridge(b) => b.poke(addr, value),
        };
        // match result {
        //     Ok(()) => println!("-> W {:08x}: {:08x}", addr, value),
        //     Err(ref e) => println!("-> W {:08x}: {:?}", addr, e),
        // }
        result
    }
}

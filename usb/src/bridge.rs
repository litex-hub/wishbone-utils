use super::config::{ConfigError, Config};
use super::usb_bridge::UsbBridge;

pub enum BridgeKind {
    /// Wishbone bridge
    Wishbone,

    /// GDB server
    GDB,

    /// No server
    None,
}

pub enum Bridge {
    UsbBridge(UsbBridge),
}

#[derive(Debug)]
pub enum BridgeError {
    /// Expected one size, but got another
    LengthError(usize, usize),

    /// USB subsystem returned an error
    USBError(libusb::Error),

    /// Attempted to communicate with the bridge, but it wasn't connected
    NotConnected,
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
            Bridge::UsbBridge(b) => b.connect()
        }
    }

    pub fn peek(&self, addr: u32) -> Result<u32, BridgeError> {
        match self {
            Bridge::UsbBridge(b) => b.peek(addr),
        }
    }

    pub fn poke(&self, addr: u32, value: u32) -> Result<(), BridgeError> {
        match self {
            Bridge::UsbBridge(b) => b.poke(addr, value)
        }
    }
}
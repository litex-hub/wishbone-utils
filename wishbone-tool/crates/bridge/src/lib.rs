//! # Wishbone Bridges
//!
//! Wishbone is an internal bus that runs on-chip. It provides memory-based
//! interconnect between various hardware modules. Wishbone buses frequently
//! contain memories such as RAM and ROM, as well as memory-mapped peripherals.
//!
//! By accessing these memories remotely, a target device may be examined or
//! tested by a host.
//!
//! Wishbone may be bridged from a target to a host using a variety of protocols.
//! This library supports different protocols depending on what features are
//! enabled. By default, all supported protocols are enabled.

pub(crate) mod bridges;

#[doc(hidden)]
pub use bridges::ethernet::EthernetBridge;
#[doc(hidden)]
pub use bridges::pcie::PCIeBridge;
#[doc(hidden)]
pub use bridges::spi::SpiBridge;
#[doc(hidden)]
pub use bridges::uart::UartBridge;
#[doc(hidden)]
pub use bridges::usb::UsbBridge;

pub use bridges::ethernet::{EthernetBridgeConfig, EthernetBridgeProtocol};
pub use bridges::pcie::PCIeBridgeConfig;
pub use bridges::spi::SpiBridgeConfig;
pub use bridges::uart::UartBridgeConfig;
pub use bridges::usb::UsbBridgeConfig;

use log::debug;

use std::io;
use std::sync::{Arc, Mutex};

#[doc(hidden)]
#[derive(Clone)]
/// A `BridgeConfig` describes the configuration of a bridge that has
/// not yet been opened.
pub enum BridgeConfig {
    /// An unconfigured `BridgeConfig`. Attempts to use this will return
    /// an `Err(NoBridgeSpecified)`, so this value exists so that `Default`
    /// may be implemented.
    None,

    /// Describes a bridge that connects via Ethernet, either via UDP
    /// (for direct hardware connections) or TCP (for connecting to
    /// other Wishbone servers such as `litex_server` or `wishbone-tool`)
    EthernetBridge(EthernetBridgeConfig),

    /// Describes a connection to a device via a PCIe bridge. Unlike most
    /// other bridges, a PCIe bridge does not provide a complete view of
    /// the memory space.
    PCIeBridge(PCIeBridgeConfig),

    /// Describes a connection to a device via SPI wires.
    SpiBridge(SpiBridgeConfig),

    /// Describes a connection to a device via a serial or other UART port.
    UartBridge(UartBridgeConfig),

    /// Describes a connection to a device via USB.
    UsbBridge(UsbBridgeConfig),
}

#[derive(Clone)]
pub enum BridgeCore {
    EthernetBridge(EthernetBridge),
    PCIeBridge(PCIeBridge),
    SpiBridge(SpiBridge),
    UartBridge(UartBridge),
    UsbBridge(UsbBridge),
}

/// Bridges represent the actual connection to the device. You must create
/// a Bridge by constructing a configuration from the relevant
/// configuration type, and then calling `create()`.
///
/// For example, to create a USB bridge, use the `USBBridgeConfig` object:
///
/// ```
/// use wishbone_bridge::UsbBridgeConfig;
/// let mut bridge_config = UsbBridgeConfig::new();
/// let bridge = bridge_config.pid(Some(0x5bf0)).create().unwrap();
/// ```
#[derive(Clone)]
pub struct Bridge {
    core: BridgeCore,
    mutex: Arc<Mutex<()>>,
}

#[derive(Debug)]
pub enum BridgeError {
    /// No bridge was specified (i.e. it was None)
    NoBridgeSpecified,

    /// Expected one size, but got another
    LengthError(usize, usize),

    /// USB subsystem returned an error
    USBError(libusb_wishbone_tool::Error),

    /// std::io error
    IoError(io::Error),

    /// Attempted to communicate with the bridge, but it wasn't connected
    NotConnected,

    /// The address or path was incorrect
    InvalidAddress,

    /// We got something weird back from the bridge
    WrongResponse,

    /// Requested protocol is not supported on this platform
    #[allow(dead_code)]
    ProtocolNotSupported,

    /// We got nothing back from the bridge
    #[allow(dead_code)]
    Timeout,
}

impl ::std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        use BridgeError::*;
        match self {
            LengthError(expected, actual) => {
                write!(f, "expected {} bytes, but got {} instead", expected, actual)
            }
            USBError(e) => write!(f, "libusb error {}", e.strerror()),
            IoError(e) => write!(f, "io error {}", e),
            NoBridgeSpecified => write!(f, "no bridge was specified"),
            NotConnected => write!(f, "bridge not connected"),
            WrongResponse => write!(f, "wrong response received"),
            InvalidAddress => write!(f, "bad address or path"),
            ProtocolNotSupported => write!(f, "protocol not supported on this platform"),
            Timeout => write!(f, "connection timed out"),
        }
    }
}

impl std::convert::From<libusb_wishbone_tool::Error> for BridgeError {
    fn from(e: libusb_wishbone_tool::Error) -> BridgeError {
        BridgeError::USBError(e)
    }
}

impl std::convert::From<io::Error> for BridgeError {
    fn from(e: io::Error) -> BridgeError {
        BridgeError::IoError(e)
    }
}

impl Bridge {
    /// Create a new Bridge with the specified configuration. The new bridge
    /// starts out in a Disconnected state, but may be connecting in the background.
    /// To ensure the bridge is connected, so you must call `connect()`.
    pub(crate) fn new(bridge_cfg: BridgeConfig) -> Result<Bridge, BridgeError> {
        let mutex = Arc::new(Mutex::new(()));
        match &bridge_cfg {
            BridgeConfig::None => Err(BridgeError::NoBridgeSpecified),
            BridgeConfig::EthernetBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::EthernetBridge(EthernetBridge::new(bridge_cfg)?),
            }),
            BridgeConfig::PCIeBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::PCIeBridge(PCIeBridge::new(bridge_cfg)?),
            }),
            BridgeConfig::SpiBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::SpiBridge(SpiBridge::new(bridge_cfg)?),
            }),
            BridgeConfig::UartBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::UartBridge(UartBridge::new(bridge_cfg)?),
            }),
            BridgeConfig::UsbBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::UsbBridge(UsbBridge::new(bridge_cfg)?),
            }),
        }
    }

    /// Ensure the bridge is connected. Many bridges support performing connection
    /// in the background, and will return an error if you attempt to perform
    /// operations such as `peek()` and `poke()` with an unconnected bridge.
    /// Calling `connect()` ensures that the bridge has been established.
    pub fn connect(&self) -> Result<(), BridgeError> {
        let _mtx = self.mutex.lock().unwrap();
        match &self.core {
            BridgeCore::EthernetBridge(b) => b.connect(),
            BridgeCore::PCIeBridge(b) => b.connect(),
            BridgeCore::SpiBridge(b) => b.connect(),
            BridgeCore::UartBridge(b) => b.connect(),
            BridgeCore::UsbBridge(b) => b.connect(),
        }
    }

    pub fn peek(&self, addr: u32) -> Result<u32, BridgeError> {
        let _mtx = self.mutex.lock().unwrap();
        loop {
            let result = match &self.core {
                BridgeCore::EthernetBridge(b) => b.peek(addr),
                BridgeCore::PCIeBridge(b) => b.peek(addr),
                BridgeCore::SpiBridge(b) => b.peek(addr),
                BridgeCore::UartBridge(b) => b.peek(addr),
                BridgeCore::UsbBridge(b) => b.peek(addr),
            };
            if let Err(e) = result {
                if let BridgeError::USBError(libusb_wishbone_tool::Error::Pipe) = e {
                    debug!("USB device disconnected, forcing early return");
                    return Err(e);
                }
                debug!("Peek failed, trying again: {:?}", e);
            } else {
                return result;
            }
        }
    }

    pub fn poke(&self, addr: u32, value: u32) -> Result<(), BridgeError> {
        let _mtx = self.mutex.lock().unwrap();
        loop {
            let result = match &self.core {
                BridgeCore::EthernetBridge(b) => b.poke(addr, value),
                BridgeCore::PCIeBridge(b) => b.poke(addr, value),
                BridgeCore::SpiBridge(b) => b.poke(addr, value),
                BridgeCore::UartBridge(b) => b.poke(addr, value),
                BridgeCore::UsbBridge(b) => b.poke(addr, value),
            };
            if let Err(e) = result {
                match e {
                    BridgeError::USBError(libusb_wishbone_tool::Error::Pipe) => {
                        debug!("USB device disconnected (Windows), forcing early return");
                        return Err(e);
                    }
                    BridgeError::USBError(libusb_wishbone_tool::Error::Io) => {
                        debug!("USB device disconnected (Posix), forcing early return");
                        return Err(e);
                    }
                    _ => {}
                }
                debug!("Poke failed, trying again: {:?}", e);
            } else {
                return result;
            }
        }
    }
}

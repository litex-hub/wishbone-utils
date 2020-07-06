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
//!
//! Creating a Wishbone `Bridge` object involves first creating a configuration
//! struct that describes the connection mechanism, and then calling `.create()`
//! on that struct to create the `Bridge`. For example, to create a USB Bridge
//! using the USB PID `0x1234`, peek memory at address 0, and poke the value
//! `0x12345678` into address `0x20000000`, you would use a `UsbBridge` like this:
//!
//! ```no_run
//! use wishbone_bridge::UsbBridge;
//! let bridge = UsbBridge::new().pid(0x1234).create().unwrap();
//! println!("Memory at address 0: {:08x}", bridge.peek(0).unwrap());
//! bridge.poke(0x2000_0000, 0x1234_5678).unwrap();
//! ```
//!
//! Creating other bridges is done in a similar manner -- see their individual
//! pages for more information.


pub(crate) mod bridges;

#[doc(hidden)]
pub use bridges::ethernet::EthernetBridgeInner;
#[doc(hidden)]
pub use bridges::pcie::PCIeBridgeInner;
#[doc(hidden)]
pub use bridges::spi::SpiBridgeInner;
#[doc(hidden)]
pub use bridges::uart::UartBridgeInner;
#[doc(hidden)]
pub use bridges::usb::UsbBridgeInner;

pub use bridges::ethernet::{EthernetBridge, EthernetBridgeProtocol};
pub use bridges::pcie::PCIeBridge;
pub use bridges::spi::SpiBridge;
pub use bridges::uart::UartBridge;
pub use bridges::usb::UsbBridge;

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
    EthernetBridge(EthernetBridge),

    /// Describes a connection to a device via a PCIe bridge. Unlike most
    /// other bridges, a PCIe bridge does not provide a complete view of
    /// the memory space.
    PCIeBridge(PCIeBridge),

    /// Describes a connection to a device via SPI wires.
    SpiBridge(SpiBridge),

    /// Describes a connection to a device via a serial or other UART port.
    UartBridge(UartBridge),

    /// Describes a connection to a device via USB.
    UsbBridge(UsbBridge),
}

#[doc(hidden)]
#[derive(Clone)]
pub enum BridgeCore {
    EthernetBridge(EthernetBridgeInner),
    PCIeBridge(PCIeBridgeInner),
    SpiBridge(SpiBridgeInner),
    UartBridge(UartBridgeInner),
    UsbBridge(UsbBridgeInner),
}

/// Bridges represent the actual connection to the device. You must create
/// a Bridge by constructing a configuration from the relevant
/// configuration type, and then calling `create()`.
///
/// For example, to create a USB bridge, use the `UsbBridge` object:
///
/// ```
/// use wishbone_bridge::UsbBridge;
/// let mut bridge_config = UsbBridge::new();
/// let bridge = bridge_config.pid(0x1234).create().unwrap();
/// ```
#[derive(Clone)]
pub struct Bridge {
    core: BridgeCore,
    mutex: Arc<Mutex<()>>,
}

/// Errors that are generated while creating or using the Wishbone Bridge.
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
                core: BridgeCore::EthernetBridge(EthernetBridgeInner::new(bridge_cfg)?),
            }),
            BridgeConfig::PCIeBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::PCIeBridge(PCIeBridgeInner::new(bridge_cfg)?),
            }),
            BridgeConfig::SpiBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::SpiBridge(SpiBridgeInner::new(bridge_cfg)?),
            }),
            BridgeConfig::UartBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::UartBridge(UartBridgeInner::new(bridge_cfg)?),
            }),
            BridgeConfig::UsbBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::UsbBridge(UsbBridgeInner::new(bridge_cfg)?),
            }),
        }
    }

    /// Ensure the bridge is connected. Many bridges support performing connection
    /// in the background, so calling `connect()` ensures that the bridge has been
    /// established.
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

    /// Read a single 32-bit value from the target device.
    /// ```no_run
    /// use wishbone_bridge::UsbBridge;
    /// let mut bridge_config = UsbBridge::new();
    /// let bridge = bridge_config.pid(0x5bf0).create().unwrap();
    /// println!("The value at address 0 is: {:08x}", bridge.peek(0).unwrap());
    /// ```
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

    /// Write a single 32-bit value into the specified address.
    /// ```no_run
    /// use wishbone_bridge::UsbBridge;
    /// let mut bridge_config = UsbBridge::new();
    /// let bridge = bridge_config.pid(0x5bf0).create().unwrap();
    /// // Poke 0x12345678 into the target device at address 0
    /// bridge.poke(0, 0x12345678).unwrap();
    /// ```
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

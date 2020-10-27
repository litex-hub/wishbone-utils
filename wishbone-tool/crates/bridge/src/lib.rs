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

#[cfg(not(any(
    feature = "pcie",
    feature = "uart",
    feature = "spi",
    feature = "ethernet",
    feature = "usb"
)))]
compile_error!("Must enable at least one bridge type: pcie, uart, spi, ethernet, or usb");

pub(crate) mod bridges;

#[doc(hidden)]
#[cfg(feature = "ethernet")]
pub use bridges::ethernet::EthernetBridgeInner;
#[doc(hidden)]
#[cfg(feature = "pcie")]
pub use bridges::pcie::PCIeBridgeInner;
#[doc(hidden)]
#[cfg(feature = "spi")]
pub use bridges::spi::SpiBridgeInner;
#[doc(hidden)]
#[cfg(feature = "uart")]
pub use bridges::uart::UartBridgeInner;
#[doc(hidden)]
#[cfg(feature = "usb")]
pub use bridges::usb::UsbBridgeInner;

#[cfg(feature = "ethernet")]
pub use bridges::ethernet::{EthernetBridge, EthernetBridgeProtocol};
#[cfg(feature = "pcie")]
pub use bridges::pcie::PCIeBridge;
#[cfg(feature = "spi")]
pub use bridges::spi::SpiBridge;
#[cfg(feature = "uart")]
pub use bridges::uart::UartBridge;
#[cfg(feature = "usb")]
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
    #[cfg(feature = "ethernet")]
    EthernetBridge(EthernetBridge),

    /// Describes a connection to a device via a PCIe bridge. Unlike most
    /// other bridges, a PCIe bridge does not provide a complete view of
    /// the memory space.
    #[cfg(feature = "pcie")]
    PCIeBridge(PCIeBridge),

    /// Describes a connection to a device via SPI wires.
    #[cfg(feature = "spi")]
    SpiBridge(SpiBridge),

    /// Describes a connection to a device via a serial or other UART port.
    #[cfg(feature = "uart")]
    UartBridge(UartBridge),

    /// Describes a connection to a device via USB.
    #[cfg(feature = "usb")]
    UsbBridge(UsbBridge),
}

#[doc(hidden)]
#[derive(Clone)]
pub enum BridgeCore {
    #[cfg(feature = "ethernet")]
    EthernetBridge(EthernetBridgeInner),
    #[cfg(feature = "pcie")]
    PCIeBridge(PCIeBridgeInner),
    #[cfg(feature = "spi")]
    SpiBridge(SpiBridgeInner),
    #[cfg(feature = "uart")]
    UartBridge(UartBridgeInner),
    #[cfg(feature = "usb")]
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
    /// Implementation-specific bridge core
    core: BridgeCore,

    /// Current offset for `Read` and `Write` operations
    offset: usize,

    /// A Mutex to enforce only a single operation at a time
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
    #[cfg(feature = "usb")]
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
            #[cfg(feature = "usb")]
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

#[cfg(feature = "usb")]
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
            #[cfg(feature = "ethernet")]
            BridgeConfig::EthernetBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::EthernetBridge(EthernetBridgeInner::new(bridge_cfg)?),
                offset: 0,
            }),
            #[cfg(feature = "pcie")]
            BridgeConfig::PCIeBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::PCIeBridge(PCIeBridgeInner::new(bridge_cfg)?),
                offset: 0,
            }),
            #[cfg(feature = "spi")]
            BridgeConfig::SpiBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::SpiBridge(SpiBridgeInner::new(bridge_cfg)?),
                offset: 0,
            }),
            #[cfg(feature = "uart")]
            BridgeConfig::UartBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::UartBridge(UartBridgeInner::new(bridge_cfg)?),
                offset: 0,
            }),
            #[cfg(feature = "usb")]
            BridgeConfig::UsbBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::UsbBridge(UsbBridgeInner::new(bridge_cfg)?),
                offset: 0,
            }),
        }
    }

    /// Ensure the bridge is connected. Many bridges support performing connection
    /// in the background, so calling `connect()` ensures that the bridge has been
    /// established.
    pub fn connect(&self) -> Result<(), BridgeError> {
        let _mtx = self.mutex.lock().unwrap();
        match &self.core {
            #[cfg(feature = "ethernet")]
            BridgeCore::EthernetBridge(b) => b.connect(),
            #[cfg(feature = "pcie")]
            BridgeCore::PCIeBridge(b) => b.connect(),
            #[cfg(feature = "spi")]
            BridgeCore::SpiBridge(b) => b.connect(),
            #[cfg(feature = "uart")]
            BridgeCore::UartBridge(b) => b.connect(),
            #[cfg(feature = "usb")]
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
                #[cfg(feature = "ethernet")]
                BridgeCore::EthernetBridge(b) => b.peek(addr),
                #[cfg(feature = "pcie")]
                BridgeCore::PCIeBridge(b) => b.peek(addr),
                #[cfg(feature = "spi")]
                BridgeCore::SpiBridge(b) => b.peek(addr),
                #[cfg(feature = "uart")]
                BridgeCore::UartBridge(b) => b.peek(addr),
                #[cfg(feature = "usb")]
                BridgeCore::UsbBridge(b) => b.peek(addr),
            };
            #[allow(unreachable_code)] // Only possible when no features are enabled (compile error)
            if let Err(e) = result {
                #[cfg(feature = "usb")]
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
                #[cfg(feature = "ethernet")]
                BridgeCore::EthernetBridge(b) => b.poke(addr, value),
                #[cfg(feature = "pcie")]
                BridgeCore::PCIeBridge(b) => b.poke(addr, value),
                #[cfg(feature = "spi")]
                BridgeCore::SpiBridge(b) => b.poke(addr, value),
                #[cfg(feature = "uart")]
                BridgeCore::UartBridge(b) => b.poke(addr, value),
                #[cfg(feature = "usb")]
                BridgeCore::UsbBridge(b) => b.poke(addr, value),
            };
            #[allow(unreachable_code)] // Only possible when no features are enabled (compile error)
            if let Err(e) = result {
                match e {
                    #[cfg(feature = "usb")]
                    BridgeError::USBError(libusb_wishbone_tool::Error::Pipe) => {
                        debug!("USB device disconnected (Windows), forcing early return");
                        return Err(e);
                    }
                    #[cfg(feature = "usb")]
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

    pub fn burst_read(&self, addr: u32, length: u32) -> Result<Vec<u8>, BridgeError> {
        let _mtx = self.mutex.lock().unwrap();
        loop {
            let result = match &self.core {
                #[cfg(feature = "ethernet")]
                BridgeCore::EthernetBridge(_b) => return Err(BridgeError::ProtocolNotSupported),
                #[cfg(feature = "pcie")]
                BridgeCore::PCIeBridge(_b) => return Err(BridgeError::ProtocolNotSupported),
                #[cfg(feature = "spi")]
                BridgeCore::SpiBridge(_b) => return Err(BridgeError::ProtocolNotSupported),
                #[cfg(feature = "uart")]
                BridgeCore::UartBridge(_b) => return Err(BridgeError::ProtocolNotSupported),
                #[cfg(feature = "usb")]
                BridgeCore::UsbBridge(b) => b.burst_read(addr, length),
            };
            #[allow(unreachable_code)] // Only possible when no features are enabled (compile error)
            if let Err(e) = result {
                #[cfg(feature = "usb")]
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

    pub fn burst_write(&self, addr: u32, data: &Vec<u8>) -> Result<(), BridgeError> {
        let _mtx = self.mutex.lock().unwrap();
        loop {
            let result = match &self.core {
                #[cfg(feature = "ethernet")]
                BridgeCore::EthernetBridge(_b) => return Err(BridgeError::ProtocolNotSupported),
                #[cfg(feature = "pcie")]
                BridgeCore::PCIeBridge(_b) => return Err(BridgeError::ProtocolNotSupported),
                #[cfg(feature = "spi")]
                BridgeCore::SpiBridge(_b) => return Err(BridgeError::ProtocolNotSupported),
                #[cfg(feature = "uart")]
                BridgeCore::UartBridge(_b) => return Err(BridgeError::ProtocolNotSupported),
                #[cfg(feature = "usb")]
                BridgeCore::UsbBridge(b) => b.burst_write(addr, data),
            };
            #[allow(unreachable_code)] // Only possible when no features are enabled (compile error)
            if let Err(e) = result {
                #[cfg(feature = "usb")]
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
}

impl std::io::Read for Bridge {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let _mtx = self.mutex.lock().unwrap();
        let addr = self.offset as _;
        use std::convert::TryInto;
        use std::io::{Error, ErrorKind};

        fn fill_array(src: &[u8], dest: &mut [u8]) -> usize {
            let mut fill_bytes = 0;
            for (s, d) in src.iter().zip(dest) {
                *d = *s;
                fill_bytes += 1;
            }
            fill_bytes
        }

        let copied = match &self.core {
            #[cfg(feature = "ethernet")]
            BridgeCore::EthernetBridge(b) => {
                b.peek(addr).map(|v| fill_array(&v.to_le_bytes(), buf))
            }
            #[cfg(feature = "pcie")]
            BridgeCore::PCIeBridge(b) => b.peek(addr).map(|v| fill_array(&v.to_le_bytes(), buf)),
            #[cfg(feature = "spi")]
            BridgeCore::SpiBridge(b) => b.peek(addr).map(|v| fill_array(&v.to_le_bytes(), buf)),
            #[cfg(feature = "uart")]
            BridgeCore::UartBridge(b) => b.peek(addr).map(|v| fill_array(&v.to_le_bytes(), buf)),
            #[cfg(feature = "usb")]
            BridgeCore::UsbBridge(b) => b
                .burst_read(addr, buf.len().try_into().unwrap())
                .map(|v| fill_array(&v, buf)),
        }
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        self.offset += copied;
        Ok(copied)
    }
}

impl std::io::Seek for Bridge {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        use std::convert::TryInto;
        use std::io::{Error, ErrorKind};
        let new_offset = match pos {
            std::io::SeekFrom::End(_) => Err(Error::new(
                ErrorKind::AddrNotAvailable,
                "cannot seek from end",
            ))?,
            std::io::SeekFrom::Current(add) => {
                if add > 0 {
                    self.offset + (add as usize)
                } else {
                    self.offset - (-add as usize)
                }
            }
            std::io::SeekFrom::Start(offset) => offset as usize,
        };
        self.offset += new_offset;
        Ok(self.offset.try_into().unwrap())
    }
}

impl std::io::Write for Bridge {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        use std::convert::TryInto;
        use std::io::{Error, ErrorKind};
        let _mtx = self.mutex.lock().unwrap();

        fn slice_to_u32(buf: &[u8]) -> std::io::Result<u32> {
            if buf.len() < 3 {
                Err(Error::new(
                    ErrorKind::InvalidData,
                    "data not a multiple of 4 bytes",
                ))?;
            }
            Ok(u32::from_le_bytes(buf[0..3].try_into().unwrap()))
        }

        let addr = self.offset as _;
        let bytes_written = match &self.core {
            #[cfg(feature = "ethernet")]
            BridgeCore::EthernetBridge(_) => self.poke(addr, slice_to_u32(buf)?).map(|_| 4),
            #[cfg(feature = "pcie")]
            BridgeCore::PCIeBridge(_) => self.poke(addr, slice_to_u32(buf)?).map(|_| 4),
            #[cfg(feature = "spi")]
            BridgeCore::SpiBridge(_) => self.poke(addr, slice_to_u32(buf)?).map(|_| 4),
            #[cfg(feature = "uart")]
            BridgeCore::UartBridge(_) => self.poke(addr, slice_to_u32(buf)?).map(|_| 4),
            #[cfg(feature = "usb")]
            BridgeCore::UsbBridge(b) => b.burst_write(addr, buf).map(|_| buf.len()),
        }
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        self.offset += bytes_written;
        Ok(bytes_written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

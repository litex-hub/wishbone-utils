mod ethernet;
mod pcie;
mod spi;
mod uart;
mod usb;

pub use ethernet::{EthernetBridge, EthernetBridgeConfig, EthernetBridgeProtocol};
pub use pcie::PCIeBridge;
pub use spi::{SpiBridge, SpiBridgeConfig};
pub use uart::{UartBridge, UartBridgeConfig};
pub use usb::{UsbBridge, UsbBridgeConfig};

use log::debug;

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub enum BridgeKind {
    None,
    EthernetBridge(EthernetBridgeConfig),
    PCIeBridge(PathBuf),
    SpiBridge(SpiBridgeConfig),
    UartBridge(UartBridgeConfig),
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
    pub fn new(bridge_cfg: &BridgeKind) -> Result<Bridge, BridgeError> {
        let mutex = Arc::new(Mutex::new(()));
        match bridge_cfg {
            BridgeKind::None => Err(BridgeError::NoBridgeSpecified),
            BridgeKind::EthernetBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::EthernetBridge(EthernetBridge::new(bridge_cfg)?),
            }),
            BridgeKind::PCIeBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::PCIeBridge(PCIeBridge::new(bridge_cfg)?),
            }),
            BridgeKind::SpiBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::SpiBridge(SpiBridge::new(bridge_cfg)?),
            }),
            BridgeKind::UartBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::UartBridge(UartBridge::new(bridge_cfg)?),
            }),
            BridgeKind::UsbBridge(bridge_cfg) => Ok(Bridge {
                mutex,
                core: BridgeCore::UsbBridge(UsbBridge::new(bridge_cfg)?),
            }),
        }
    }

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

    pub fn mutex(&self) -> &Arc<Mutex<()>> {
        match &self.core {
            BridgeCore::EthernetBridge(b) => b.mutex(),
            BridgeCore::PCIeBridge(b) => b.mutex(),
            BridgeCore::SpiBridge(b) => b.mutex(),
            BridgeCore::UartBridge(b) => b.mutex(),
            BridgeCore::UsbBridge(b) => b.mutex(),
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

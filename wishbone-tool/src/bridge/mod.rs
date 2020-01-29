pub mod uart;
pub mod usb;
pub mod spi;
pub mod ethernet;

use crate::config::Config;
use usb::UsbBridge;
use uart::UartBridge;
use spi::SpiBridge;
use ethernet::EthernetBridge;
use log::debug;

use std::sync::{Arc, Mutex};
use std::io;

#[derive(Clone)]
pub enum BridgeKind {
    UsbBridge,
    UartBridge,
    SpiBridge,
    EthernetBridge,
}

#[derive(Clone)]
pub enum BridgeCore {
    UsbBridge(UsbBridge),
    UartBridge(UartBridge),
    SpiBridge(SpiBridge),
    EthernetBridge(EthernetBridge),
}

#[derive(Clone)]
pub struct Bridge {
    core: BridgeCore,
    mutex: Arc<Mutex<()>>,
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

    /// We got nothing back from the bridge
    #[allow(dead_code)]
    Timeout,
}

impl ::std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        use BridgeError::*;
        match self {
            LengthError(expected, actual) => write!(f, "expected {} bytes, but got {} instead", expected, actual),
            USBError(e) => write!(f, "libusb error {}", e.strerror()),
            IoError(e) => write!(f, "io error {}", e),
            NotConnected => write!(f, "bridge not connected"),
            WrongResponse => write!(f, "wrong response received"),
            Timeout => write!(f, "connection timed out"),
        }
    }
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
        let mutex = Arc::new(Mutex::new(()));
        match cfg.bridge_kind {
            BridgeKind::UartBridge => Ok(Bridge { mutex, core: BridgeCore::UartBridge(UartBridge::new(cfg)?) } ),
            BridgeKind::UsbBridge => Ok(Bridge { mutex, core: BridgeCore::UsbBridge(UsbBridge::new(cfg)?) } ),
            BridgeKind::SpiBridge => Ok(Bridge { mutex, core: BridgeCore::SpiBridge(SpiBridge::new(cfg)?) } ),
            BridgeKind::EthernetBridge => Ok(Bridge { mutex, core: BridgeCore::EthernetBridge(EthernetBridge::new(cfg)?) } ),
        }
    }

    pub fn connect(&self) -> Result<(), BridgeError> {
        let _mtx = self.mutex.lock().unwrap();
        match &self.core{
            BridgeCore::UsbBridge(b) => b.connect(),
            BridgeCore::UartBridge(b) => b.connect(),
            BridgeCore::SpiBridge(b) => b.connect(),
            BridgeCore::EthernetBridge(b) => b.connect(),
        }
    }

    pub fn mutex(&self) -> &Arc<Mutex<()>> {
        match &self.core {
            BridgeCore::UsbBridge(b) => b.mutex(),
            BridgeCore::UartBridge(b) => b.mutex(),
            BridgeCore::SpiBridge(b) => b.mutex(),
            BridgeCore::EthernetBridge(b) => b.mutex(),
        }
    }

    pub fn peek(&self, addr: u32) -> Result<u32, BridgeError> {
        let _mtx = self.mutex.lock().unwrap();
        loop {
            let result = match &self.core {
                BridgeCore::UsbBridge(b) => b.peek(addr),
                BridgeCore::UartBridge(b) => b.peek(addr),
                BridgeCore::SpiBridge(b) => b.peek(addr),
                BridgeCore::EthernetBridge(b) => b.peek(addr),
            };
            if let Err(e) = result {
                match e {
                    BridgeError::USBError(libusb::Error::Pipe) => {
                        debug!("USB device disconnected, forcing early return");
                        return Err(e)
                    },
                    _ => {},
                }
                debug!("Peek failed, trying again: {:?}", e);
            }
            else {
                return result;
            }
        }
    }

    pub fn poke(&self, addr: u32, value: u32) -> Result<(), BridgeError> {
        let _mtx = self.mutex.lock().unwrap();
        loop {
            let result = match &self.core {
                BridgeCore::UsbBridge(b) => b.poke(addr, value),
                BridgeCore::UartBridge(b) => b.poke(addr, value),
                BridgeCore::SpiBridge(b) => b.poke(addr, value),
                BridgeCore::EthernetBridge(b) => b.poke(addr, value),
            };
            if let Err(e) = result {
                match e {
                    BridgeError::USBError(libusb::Error::Pipe) => {
                        debug!("USB device disconnected (Windows), forcing early return");
                        return Err(e);
                    }
                    BridgeError::USBError(libusb::Error::Io) => {
                        debug!("USB device disconnected (Posix), forcing early return")
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

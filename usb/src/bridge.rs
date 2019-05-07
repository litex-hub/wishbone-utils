use super::config::ConfigError;

pub enum BridgeKind {
    /// Wishbone bridge
    Wishbone,

    /// GDB server
    GDB,

    /// No server
    None,
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

use crate::bridge::{BridgeError, SpiBridgeConfig};
use std::sync::{Arc, Mutex};

#[allow(dead_code)]
#[derive(Clone)]
pub struct SpiBridge;

impl SpiBridge {
    pub fn new(_cfg: &SpiBridgeConfig) -> Result<Self, BridgeError> {
        Err(BridgeError::ProtocolNotSupported)
    }

    pub fn mutex(&self) -> &Arc<Mutex<()>> {
        unimplemented!("SPI is unimplemented on this platform");
    }

    pub fn connect(&self) -> Result<(), BridgeError> {
        unimplemented!("SPI is unimplemented on this platform");
    }

    pub fn poke(&self, _addr: u32, _value: u32) -> Result<(), BridgeError> {
        unimplemented!("SPI is unimplemented on this platform");
    }

    pub fn peek(&self, _addr: u32) -> Result<u32, BridgeError> {
        unimplemented!("SPI is unimplemented on this platform");
    }
}

use crate::{BridgeError, SpiBridge};
use std::sync::{Arc, Mutex};

#[allow(dead_code)]
#[derive(Clone)]
pub struct SpiBridgeInner;

impl SpiBridgeInner {
    pub fn new(_cfg: &SpiBridge) -> Result<Self, BridgeError> {
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

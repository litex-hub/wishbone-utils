use std::sync::{Arc, Mutex};
use crate::bridge::BridgeError;
use crate::config::Config;

#[allow(dead_code)]
#[derive(Clone)]
pub struct SpiBridge;

impl SpiBridge {
    pub fn new(_cfg: &Config) -> Result<Self, BridgeError> {
        unimplemented!("SPI is unimplemented on this platform");
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
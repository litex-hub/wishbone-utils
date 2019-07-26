use clap::ArgMatches;
use super::bridge::{BridgeKind, BridgeServerKind};
use super::utils::{parse_u16, parse_u32};

pub struct Config {
    pub usb_pid: Option<u16>,
    pub usb_vid: Option<u16>,
    pub memory_address: Option<u32>,
    pub memory_value: Option<u32>,
    pub bridge_server_kind: BridgeServerKind,
    pub bridge_kind: BridgeKind,
    pub serial_port: Option<String>,
    pub serial_baud: Option<usize>,
    pub bind_addr: String,
    pub bind_port: u32,
    pub random_loops: Option<u32>,
}

#[derive(Debug)]
pub enum ConfigError {
    /// Couldn't parse string as number
    NumberParseError(std::num::ParseIntError),

    /// Specified a bridge kind that we didn't recognize
    UnknownBridgeServerKind(String),
}

impl std::convert::From<std::num::ParseIntError> for ConfigError {
    fn from(e: std::num::ParseIntError) -> Self {
        ConfigError::NumberParseError(e)
    }
}

impl Config {
    pub fn parse(matches: ArgMatches) -> Result<Self, ConfigError> {
        let mut bridge_kind = BridgeKind::UsbBridge;

        let usb_vid = if let Some(vid) = matches.value_of("vid") {
            Some(parse_u16(vid)?)
        } else {
            None
        };

        let usb_pid = if let Some(pid) = matches.value_of("pid") {
            Some(parse_u16(pid)?)
        } else {
            None
        };

        let serial_port = if let Some(port) = matches.value_of("serial") {
            bridge_kind = BridgeKind::UartBridge;
            Some(port.to_owned())
        } else {
            None
        };

        let serial_baud = if let Some(baud) = matches.value_of("baud") {
            Some(parse_u32(baud)? as usize)
        } else {
            None
        };

        let memory_address = if let Some(addr) = matches.value_of("address") {
            Some(parse_u32(addr)?)
        } else {
            None
        };

        let memory_value = if let Some(v) = matches.value_of("value") {
            Some(parse_u32(v)?)
        } else {
            None
        };

        let bind_port = if let Some(port) = matches.value_of("port") {
            parse_u32(port)?
        } else {
            3333
        };

        let bind_addr = if let Some(addr) = matches.value_of("bind-addr") {
            addr.to_owned()
        } else {
            "127.0.0.1".to_owned()
        };

        let bridge_server_kind = BridgeServerKind::from_string(&matches.value_of("bridge-kind"))?;

        let random_loops = if let Some(random_loops) = matches.value_of("random-loops") {
            Some(parse_u32(random_loops)?)
        } else {
            None
        };

        Ok(Config {
            usb_pid,
            usb_vid,
            serial_port,
            serial_baud,
            memory_address,
            memory_value,
            bridge_server_kind,
            bridge_kind,
            bind_port,
            bind_addr,
            random_loops,
        })
    }
}
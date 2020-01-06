use std::collections::HashMap;
use std::fs::File;

use crate::bridge::spi::SpiPins;
use crate::bridge::BridgeKind;
use crate::server::ServerKind;
use clap::ArgMatches;
use csv;

#[derive(Debug)]
pub enum ConfigError {
    /// Couldn't parse string as number
    NumberParseError(String, std::num::ParseIntError),

    /// Specified a bridge kind that we didn't recognize
    UnknownServerKind(String),

    /// Specified SPI pinspec was invalid
    SpiParseError(String),

    /// No operation was specified
    NoOperationSpecified,
}

pub fn get_base(value: &str) -> (&str, u32) {
    if value.starts_with("0x") {
        (value.trim_start_matches("0x"), 16)
    } else if value.starts_with("0X") {
        (value.trim_start_matches("0X"), 16)
    } else if value.starts_with("0b") {
        (value.trim_start_matches("0b"), 2)
    } else if value.starts_with("0B") {
        (value.trim_start_matches("0B"), 2)
    } else if value.starts_with("0") && value != "0" {
        (value.trim_start_matches("0"), 8)
    } else {
        (value, 10)
    }
}

pub fn parse_u16(value: &str) -> Result<u16, ConfigError> {
    let (value, base) = get_base(value);
    match u16::from_str_radix(value, base) {
        Ok(o) => Ok(o),
        Err(e) => Err(ConfigError::NumberParseError(value.to_owned(), e)),
    }
}

pub fn parse_u32(value: &str) -> Result<u32, ConfigError> {
    let (value, base) = get_base(value);
    match u32::from_str_radix(value, base) {
        Ok(o) => Ok(o),
        Err(e) => Err(ConfigError::NumberParseError(value.to_owned(), e)),
    }
}

pub struct Config {
    pub usb_pid: Option<u16>,
    pub usb_vid: Option<u16>,
    pub memory_address: Option<u32>,
    pub memory_value: Option<u32>,
    pub server_kind: ServerKind,
    pub bridge_kind: BridgeKind,
    pub serial_port: Option<String>,
    pub serial_baud: Option<usize>,
    pub spi_pins: Option<SpiPins>,
    pub bind_addr: String,
    pub bind_port: u32,
    pub random_loops: Option<u32>,
    pub random_address: Option<u32>,
    pub random_range: Option<u32>,
    pub messible_address: Option<u32>,
    pub register_mapping: HashMap<String, u32>,
    pub debug_offset: u32,
    pub load_name: Option<String>,
    pub load_addr: Option<u32>,
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

        let load_name = if let Some(n) = matches.value_of("load-name") {
            Some(n.to_owned())
        } else {
            None
        };

        let load_addr = if let Some(addr) = matches.value_of("load-address") {
            Some(parse_u32(addr)?)
        } else {
            None
        };

        let register_mapping = Self::parse_csr_csv(matches.value_of("csr-csv"));

        let memory_address = if let Some(addr) = matches.value_of("address") {
            if let Some(addr) = register_mapping.get(&addr.to_lowercase()) {
                Some(*addr)
            } else {
                Some(parse_u32(addr)?)
            }
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

        let spi_pins = if let Some(pins) = matches.value_of("spi-pins") {
            bridge_kind = BridgeKind::SpiBridge;
            Some(SpiPins::from_string(pins)?)
        } else {
            None
        };

        let server_kind = ServerKind::from_string(&matches.value_of("server-kind"))?;

        let random_loops = if let Some(random_loops) = matches.value_of("random-loops") {
            Some(parse_u32(random_loops)?)
        } else {
            None
        };

        let random_address = if let Some(random_address) = matches.value_of("random-address") {
            Some(parse_u32(random_address)?)
        } else {
            None
        };

        let random_range = if let Some(random_range) = matches.value_of("random-range") {
            Some(parse_u32(random_range)?)
        } else {
            None
        };

        let messible_address = if let Some(messible_address) = matches.value_of("messible-address")
        {
            Some(parse_u32(messible_address)?)
        } else {
            None
        };

        let debug_offset = if let Some(debug_offset) = matches.value_of("debug-offset")
        {
            parse_u32(debug_offset)?
        } else {
            0xf00f0000
        };

        if memory_address.is_none() && server_kind == ServerKind::None {
            Err(ConfigError::NoOperationSpecified)
        } else {
            Ok(Config {
                usb_pid,
                usb_vid,
                serial_port,
                serial_baud,
                spi_pins,
                memory_address,
                memory_value,
                server_kind,
                bridge_kind,
                bind_port,
                bind_addr,
                random_loops,
                random_address,
                random_range,
                messible_address,
                register_mapping,
                debug_offset,
                load_name,
                load_addr,
            })
        }
    }

    fn parse_csr_csv(filename: Option<&str>) -> HashMap<String, u32> {
        let mut map = HashMap::new();
        let file = match filename {
            None => return map,
            Some(s) => match File::open(s) {
                Ok(o) => o,
                Err(e) => panic!("Unable to open csr-csv file: {}", e),
            }
        };
        let mut rdr = csv::ReaderBuilder::new().flexible(true).from_reader(file);
        for result in rdr.records() {
            if let Ok(r) = result {
                if &r[0] != "csr_register" {
                    continue;
                }
                let reg_name = &r[1];
                let base_addr = match parse_u32(&r[2]) {
                    Ok(o) => o,
                    Err(e) => panic!("Couldn't parse csr-csv base address: {:?}", e),
                };
                let num_regs = match parse_u32(&r[3]) {
                    Ok(o) => o,
                    Err(e) => panic!("Couldn't parse csr-csv number of registers: {:?}", e),
                };

                // If there's only one register, add it to the map.
                // However, CSRs can span multiple registers, and do so in reverse.
                // If this is the case, create indexed offsets for those registers.
                match num_regs {
                    1 => {
                        map.insert(reg_name.to_string().to_lowercase(), base_addr);
                    },
                    n => {
                        for offset in 0..n {
                            map.insert(format!("{}{}", reg_name.to_string().to_lowercase(), n - offset - 1), base_addr+(offset*4));
                        }
                    }
                }
            }
        }
        map
    }
}

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::PathBuf;

use crate::bridge::BridgeKind;
use crate::bridge::SpiPins;
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

    /// Generic IO Error
    IoError(io::Error),

    /// The configuration doesn't make sense
    InvalidConfig(String),
}

impl std::convert::From<io::Error> for ConfigError {
    fn from(e: io::Error) -> ConfigError {
        ConfigError::IoError(e)
    }
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
    } else if value.starts_with('0') && value != "0" {
        (value.trim_start_matches('0'), 8)
    } else {
        (value, 10)
    }
}

pub fn parse_u8(value: &str) -> Result<u8, ConfigError> {
    let (value, base) = get_base(value);
    match u8::from_str_radix(value, base) {
        Ok(o) => Ok(o),
        Err(e) => Err(ConfigError::NumberParseError(value.to_owned(), e)),
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

#[derive(Clone)]
pub struct Config {
    pub usb_pid: Option<u16>,
    pub usb_vid: Option<u16>,
    pub usb_bus: Option<u8>,
    pub usb_device: Option<u8>,
    pub memory_address: Option<u32>,
    pub memory_value: Option<u32>,
    pub server_kind: Vec<ServerKind>,
    pub bridge_kind: BridgeKind,
    pub serial_port: Option<String>,
    pub serial_baud: Option<usize>,
    pub pcie_path: Option<PathBuf>,
    pub spi_pins: Option<SpiPins>,
    pub bind_addr: String,
    pub bind_port: u16,
    pub gdb_port: u16,
    pub ethernet_host: Option<String>,
    pub ethernet_port: u16,
    pub ethernet_tcp: bool,
    pub random_loops: Option<u32>,
    pub random_address: Option<u32>,
    pub random_range: Option<u32>,
    pub messible_address: Option<u32>,
    pub register_mapping: HashMap<String, u32>,
    pub debug_offset: u32,
    pub load_name: Option<String>,
    pub load_addr: Option<u32>,
    pub terminal_mouse: bool,
}

impl Config {
    pub fn parse(matches: ArgMatches) -> Result<Self, ConfigError> {
        let mut bridge_kind = BridgeKind::UsbBridge;
        let mut server_kind = vec![];

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

        let usb_bus = if let Some(bus) = matches.value_of("bus") {
            Some(parse_u8(bus)?)
        } else {
            None
        };

        let usb_device = if let Some(device) = matches.value_of("device") {
            Some(parse_u8(device)?)
        } else {
            None
        };
        // TODO: add parsing for bus and address here

        let serial_port = if let Some(port) = matches.value_of("serial") {
            bridge_kind = BridgeKind::UartBridge;
            // Strip off the trailing ":" on Windows, since it's confusing
            if cfg!(windows) && port.ends_with(':') {
                Some(port.get(0..port.len() - 1).unwrap_or("").to_owned())
            } else {
                Some(port.to_owned())
            }
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
            if load_name.is_none() {
                server_kind.push(ServerKind::MemoryAccess);
            }
            Some(parse_u32(addr)?)
        } else {
            None
        };

        let memory_value = if let Some(v) = matches.value_of("value") {
            Some(parse_u32(v)?)
        } else {
            None
        };

        // unwrap() is safe because there is a default value
        let gdb_port = parse_u16(matches.value_of("gdb-port").unwrap())?;
        let bind_port = parse_u16(matches.value_of("wishbone-port").unwrap())?;
        let ethernet_port = parse_u16(matches.value_of("ethernet-port").unwrap())?;

        let bind_addr = if let Some(addr) = matches.value_of("bind-addr") {
            addr.to_owned()
        } else {
            "127.0.0.1".to_owned()
        };

        let ethernet_host = if let Some(host) = matches.value_of("ethernet-host") {
            bridge_kind = BridgeKind::EthernetBridge;
            Some(host.to_owned())
        } else {
            None
        };

        let ethernet_tcp = matches.is_present("ethernet-tcp");

        let pcie_path = matches.value_of("pcie-bar").map(|path| {
            bridge_kind = BridgeKind::PCIeBridge;
            PathBuf::from(path)
        });

        let spi_pins = if let Some(pins) = matches.value_of("spi-pins") {
            bridge_kind = BridgeKind::SpiBridge;
            Some(SpiPins::from_string(pins)?)
        } else {
            None
        };

        if let Some(server_kinds) = matches.values_of("server-kind") {
            for sk in server_kinds {
                server_kind.push(ServerKind::from_string(sk)?);
            }
        }

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

        let register_mapping = Self::parse_csr_csv(matches.value_of("csr-csv"))?;

        let messible_address = if let Some(messible_address) = matches.value_of("messible-address")
        {
            Some(parse_u32(messible_address)?)
        } else if let Some(base) = register_mapping.get("messible_out") {
            Some(*base)
        } else {
            None
        };

        let debug_offset = if let Some(debug_offset) = matches.value_of("debug-offset") {
            parse_u32(debug_offset)?
        } else if let Some(debug_offset) = register_mapping.get("vexriscv_debug") {
            *debug_offset
        } else {
            0xf00f_0000
        };

        let memory_address = if let Some(addr) = matches.value_of("address") {
            if let Some(addr) = register_mapping.get(&addr.to_lowercase()) {
                Some(*addr)
            } else {
                Some(parse_u32(addr)?)
            }
        } else {
            None
        };

        if server_kind.is_empty() {
            if memory_address.is_none() {
                return Err(ConfigError::NoOperationSpecified);
            }
            server_kind.push(ServerKind::MemoryAccess);
        }

        // Validate the configuration is correct
        if matches.value_of("csr-csv").is_some() {
            if server_kind.contains(&ServerKind::GDB) {
                // You asked for --server gdb but no vexriscv jtag interfaces is found in the csr.csv file it should complain.
                if !register_mapping.contains_key("vexriscv_debug") {
                    return Err(ConfigError::InvalidConfig(
                        "GDB specified but no vexriscv address present in csv file".to_owned(),
                    ));
                }
            }
            if server_kind.contains(&ServerKind::Terminal) {
                // You asked for --server terminal but no uart is found in the csr.csv file it should complain.
                if !(register_mapping.contains_key("uart_xover_rxtx")
                    && register_mapping.contains_key("uart_xover_rxempty")
                    && register_mapping.contains_key("uart_xover_ev_pending"))
                {
                    return Err(ConfigError::InvalidConfig(
                        "Terminal specified, but no xover uart addresses present in csv file"
                            .to_owned(),
                    ));
                }
            }
        }

        let terminal_mouse = matches.is_present("terminal-mouse") || cfg!(windows);

        Ok(Config {
            usb_pid,
            usb_vid,
            usb_bus,
            usb_device,
            serial_port,
            serial_baud,
            spi_pins,
            pcie_path,
            memory_address,
            memory_value,
            server_kind,
            bridge_kind,
            bind_port,
            bind_addr,
            gdb_port,
            random_loops,
            random_address,
            random_range,
            messible_address,
            register_mapping,
            debug_offset,
            load_name,
            load_addr,
            ethernet_host,
            ethernet_port,
            ethernet_tcp,
            terminal_mouse,
        })
    }

    fn parse_csr_csv(filename: Option<&str>) -> Result<HashMap<String, u32>, ConfigError> {
        let mut map = HashMap::new();
        let file = match filename {
            None => return Ok(map),
            Some(s) => File::open(s)?,
        };
        let mut rdr = csv::ReaderBuilder::new().flexible(true).from_reader(file);
        for result in rdr.records() {
            if let Ok(r) = result {
                match &r[0] {
                    "csr_register" => {
                        let reg_name = &r[1];
                        let base_addr = parse_u32(&r[2])?;
                        let num_regs = parse_u32(&r[3])?;

                        // If there's only one register, add it to the map.
                        // However, CSRs can span multiple registers, and do so in reverse.
                        // If this is the case, create indexed offsets for those registers.
                        match num_regs {
                            1 => {
                                map.insert(reg_name.to_string().to_lowercase(), base_addr);
                            }
                            n => {
                                map.insert(reg_name.to_string().to_lowercase(), base_addr);
                                for offset in 0..n {
                                    map.insert(
                                        format!(
                                            "{}{}",
                                            reg_name.to_string().to_lowercase(),
                                            n - offset - 1
                                        ),
                                        base_addr + (offset * 4),
                                    );
                                }
                            }
                        }
                    }
                    "memory_region" => {
                        let region = &r[1];
                        let base_addr = parse_u32(&r[2])?;
                        map.insert(region.to_string().to_lowercase(), base_addr);
                    }
                    _ => (),
                };
            }
        }
        Ok(map)
    }
}

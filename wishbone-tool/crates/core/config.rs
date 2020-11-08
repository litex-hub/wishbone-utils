use std::collections::HashMap;
use std::fs::File;
use std::io;

use crate::server::ServerKind;
use clap::ArgMatches;
use wishbone_bridge::{
    Bridge, EthernetBridge, EthernetBridgeProtocol, PCIeBridge, SpiBridge, UartBridge, UsbBridge,
};

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

    /// The specified address is outside of legal memory
    AddressOutOfRange(String),
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

pub fn parse_u32_address(value: &str, offset: u32) -> Result<Option<u32>, ConfigError> {
    let (value, base) = get_base(value);
    u32::from_str_radix(value, base)
        .map(|n| if n >= offset { Some(n - offset) } else { None })
        .or_else(|e| Err(ConfigError::NumberParseError(value.to_owned(), e)))
}

#[derive(Clone)]
pub struct Config {
    pub memory_address: Option<u32>,
    pub memory_value: Option<u32>,
    pub server_kind: Vec<ServerKind>,
    pub bind_addr: String,
    pub bind_port: u16,
    pub gdb_port: u16,
    pub random_loops: Option<u32>,
    pub random_address: Option<u32>,
    pub random_range: Option<u32>,
    pub messible_address: Option<u32>,
    pub register_mapping: HashMap<String, Option<u32>>,
    pub debug_offset: u32,
    pub load_name: Option<String>,
    pub load_addr: Option<u32>,
    pub load_flash: bool,
    pub terminal_mouse: bool,
    pub burst_length: u32,
    pub hexdump: bool,
    pub burst_source: Option<String>,
    pub flash_no_reset: bool,
    pub careful_flashing: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            memory_address: None,
            memory_value: None,
            server_kind: vec![],
            bind_addr: "127.0.0.1".to_owned(),
            bind_port: 1234,
            gdb_port: 3333,
            random_loops: None,
            random_address: None,
            random_range: None,
            messible_address: None,
            register_mapping: HashMap::new(),
            debug_offset: 0,
            load_name: None,
            load_addr: None,
            load_flash: false,
            terminal_mouse: false,
            burst_length: 4,
            hexdump: false,
            burst_source: None,
            flash_no_reset: false,
            careful_flashing: false,
        }
    }
}

impl Config {
    fn create_bridge(matches: &ArgMatches) -> Result<Bridge, ConfigError> {
        // If SPI pins are specified, then assume the bridge must be SPI.
        if let Some(pins) = matches.value_of("spi-pins") {
            return SpiBridge::new(pins)
                .or_else(|e| Err(ConfigError::SpiParseError(e)))?
                .create()
                .map_err(|e| {
                    ConfigError::InvalidConfig(format!("unable to create spi bridge: {}", e))
                });
        }

        // UART bridge config
        if let Some(port) = matches.value_of("serial") {
            // Strip off the trailing ":" on Windows, since it's confusing
            let serial_port = if cfg!(windows) && port.ends_with(':') {
                port.get(0..port.len() - 1).unwrap_or("")
            } else {
                port
            };
            let mut uart_config = UartBridge::new(serial_port).or_else(|e| {
                Err(ConfigError::InvalidConfig(format!(
                    "invalid serial port: {}",
                    e
                )))
            })?;

            if let Some(baud) = matches.value_of("baud") {
                uart_config.baud(parse_u32(baud)?);
            }

            return uart_config.create().map_err(|e| {
                ConfigError::InvalidConfig(format!("unable to create uart bridge: {}", e))
            });
        }

        // PCIe BAR-as-a-file
        if let Some(pcie_bar) = matches.value_of("pcie-bar") {
            return PCIeBridge::new(pcie_bar)
                .or_else(|e| {
                    Err(ConfigError::InvalidConfig(format!(
                        "invalid pcie bar: {}",
                        e
                    )))
                })?
                .create()
                .map_err(|e| {
                    ConfigError::InvalidConfig(format!("unable to create pcie bridge: {}", e))
                });
        }

        // Ethernet (TCP or UDP)
        if let Some(host) = matches.value_of("ethernet-host") {
            let ethernet_tcp = matches.is_present("ethernet-tcp");
            let ethernet_port = parse_u16(matches.value_of("ethernet-port").unwrap())?;
            let mut ebc = EthernetBridge::new(host)
                .or_else(|_| EthernetBridge::new(&format!("{}:{}", host, ethernet_port)))
                .or_else(|e| {
                    Err(ConfigError::InvalidConfig(format!(
                        "invalid ethernet address: {}",
                        e
                    )))
                })?;
            ebc.protocol(if ethernet_tcp {
                EthernetBridgeProtocol::TCP
            } else {
                EthernetBridgeProtocol::UDP
            })
            .port(ethernet_port);
            return ebc.create().map_err(|e| {
                ConfigError::InvalidConfig(format!("unable to create ethernet bridge: {}", e))
            });
        }

        // Fall back to USB
        let mut usb_config = UsbBridge::new();
        if let Some(vid) = matches.value_of("vid") {
            usb_config.vid(parse_u16(vid)?);
        }
        if let Some(pid) = matches.value_of("pid") {
            usb_config.pid(parse_u16(pid)?);
        }
        if let Some(bus) = matches.value_of("bus") {
            usb_config.bus(parse_u8(bus)?);
        }
        if let Some(device) = matches.value_of("device") {
            usb_config.device(parse_u8(device)?);
        }
        usb_config
            .create()
            .map_err(|e| ConfigError::InvalidConfig(format!("unable to create usb bridge: {}", e)))
    }

    pub fn parse(matches: ArgMatches) -> Result<(Self, Bridge), ConfigError> {
        let mut server_kind = vec![];

        let load_name = matches.value_of("load-name").map(|n| n.to_owned());
        let load_flash = matches.is_present("load-flash");
        let load_addr = if let Some(addr) = matches.value_of("load-address") {
            if load_name.is_none() & !load_flash {
                server_kind.push(ServerKind::MemoryAccess);
            }
            Some(parse_u32(addr)?)
        } else {
            None
        };
        if load_addr.is_some() & load_name.is_some() & load_flash {
            server_kind.push(ServerKind::FlashProgram);
        }

        let memory_value = matches
            .value_of("value")
            .map(|v| parse_u32(v))
            .transpose()?;

        // unwrap() is safe because there is a default value
        let gdb_port = parse_u16(matches.value_of("gdb-port").unwrap())?;
        let bind_port = parse_u16(matches.value_of("wishbone-port").unwrap())?;
        let burst_length = parse_u32(matches.value_of("burst-length").unwrap())?;

        let bind_addr = matches
            .value_of("bind-addr")
            .map(|addr| addr.to_owned())
            .unwrap_or_else(|| "127.0.0.1".to_owned());

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

        let (register_mapping, offset) = Self::parse_csr_csv(
            matches.value_of("csr-csv"),
            matches.value_of("register-offset"),
        )?;

        let messible_address = if let Some(messible_address) = matches.value_of("messible-address")
        {
            Some(
                parse_u32_address(messible_address, offset)?
                    .ok_or_else(|| ConfigError::AddressOutOfRange(messible_address.to_owned()))?,
            )
        } else if let Some(base) = register_mapping.get("messible_out") {
            Some((*base).ok_or_else(|| ConfigError::AddressOutOfRange("messible_out".to_owned()))?)
        } else {
            None
        };

        let debug_offset = if let Some(debug_offset) = matches.value_of("debug-offset") {
            parse_u32_address(debug_offset, offset)?
                .ok_or_else(|| ConfigError::AddressOutOfRange(debug_offset.to_owned()))?
        } else if let Some(debug_offset) = register_mapping.get("vexriscv_debug") {
            (*debug_offset)
                .ok_or_else(|| ConfigError::AddressOutOfRange("vexriscv_debug".to_owned()))?
        } else {
            0xf00f_0000
        };

        let memory_address = if let Some(addr) = matches.value_of("address") {
            if let Some(mapped_addr) = register_mapping.get(&addr.to_lowercase()) {
                Some(
                    (*mapped_addr)
                        .ok_or_else(|| ConfigError::AddressOutOfRange(addr.to_owned()))?,
                )
            } else {
                Some(
                    parse_u32_address(addr, offset)?
                        .ok_or_else(|| ConfigError::AddressOutOfRange(addr.to_owned()))?,
                )
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
            if server_kind.contains(&ServerKind::FlashProgram) {
                if !(register_mapping.contains_key("spinor")
                 ) {
                    return Err(ConfigError::InvalidConfig(
                        "Flash programming requested, but no spinor block present in csv file"
                            .to_owned(),
                    ));
                 }
            }
        }

        let terminal_mouse = matches.is_present("terminal-mouse") || cfg!(windows);
        let hexdump = matches.is_present("hexdump");
        let flash_no_reset = matches.is_present("flash-no-reset");
        let careful_flashing = matches.is_present("careful-flashing");

        let burst_source = matches.value_of("burst-source").map(|n| n.to_owned());

        let bridge = Self::create_bridge(&matches)?;

        Ok((
            Config {
                memory_address,
                memory_value,
                server_kind,
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
                load_flash,
                terminal_mouse,
                burst_length,
                hexdump,
                burst_source,
                flash_no_reset,
                careful_flashing,
            },
            bridge,
        ))
    }

    fn parse_csr_csv(
        filename: Option<&str>,
        offset_str: Option<&str>,
    ) -> Result<(HashMap<String, Option<u32>>, u32), ConfigError> {
        let mut map = HashMap::new();
        let file = match filename {
            None => {
                if let Some(offset_str) = offset_str {
                    return Ok((map, parse_u32(offset_str)?));
                } else {
                    return Ok((map, 0));
                }
            }
            Some(s) => File::open(s)?,
        };

        let mut offset = 0;

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
                                map.insert(reg_name.to_string().to_lowercase(), Some(base_addr));
                            }
                            n => {
                                map.insert(reg_name.to_string().to_lowercase(), Some(base_addr));
                                for logical_reg in 0..n {
                                    map.insert(
                                        format!(
                                            "{}{}",
                                            reg_name.to_string().to_lowercase(),
                                            n - logical_reg - 1
                                        ),
                                        Some(base_addr + logical_reg * 4),
                                    );
                                }
                            }
                        }
                    }
                    "memory_region" => {
                        let region = &r[1];
                        let base_addr = parse_u32(&r[2])?;
                        map.insert(region.to_string().to_lowercase(), Some(base_addr));
                    }
                    "csr_base" => {
                        let region = &r[1];
                        let base_addr = parse_u32(&r[2])?;
                        map.insert(region.to_string().to_lowercase(), Some(base_addr));
                    }
                    _ => (),
                };
            }
        }

        // Now that we have everything loaded into the hashmap, see if we need to offset values.
        if let Some(offset_str) = offset_str {
            if let Some(offset_value) = map.get(offset_str) {
                // All values in the map should be non-None now, since we haven't updated the offsets yet.
                offset =
                    offset_value.expect("offset_value was None even though it was in the array");
            } else {
                offset = parse_u32(offset_str)?;
            }
            for (_key, val) in map.iter_mut() {
                // If there's an offset and it's outside of the range of
                // this CSR, just ignore the CSR.
                if val.expect("val was None") < offset {
                    *val = None;
                } else {
                    *val.as_mut().unwrap() -= offset;
                }
            }
        }
        Ok((map, offset))
    }
}

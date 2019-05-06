extern crate clap;
extern crate libusb;

mod gdb;

use clap::{App, Arg, ArgMatches};
use std::num::ParseIntError;
use std::time::Duration;

struct WishboneBridge<'a> {
    // usb_ctx: libusb::Context,
    usb: Option<libusb::DeviceHandle<'a>>,
}

enum BridgeKind {
    /// Wishbone bridge
    Wishbone,

    /// GDB server
    GDB,

    /// No server
    None,
}

#[derive(Debug)]
enum BridgeError {
    /// Expected one size, but got another
    LengthError(usize, usize),

    /// USB subsystem returned an error
    USBError(libusb::Error),
}

pub struct Config {
    usb_pid: Option<u16>,
    usb_vid: Option<u16>,
    memory_address: Option<u32>,
    memory_value: Option<u32>,
    bridge_kind: BridgeKind,
    bind_addr: String,
    bind_port: u32,
}

#[derive(Debug)]
enum ConfigError {
    /// Couldn't parse string as number
    NumberParseError(std::num::ParseIntError),

    /// Specified a bridge kind that we didn't recognize
    UnknownBridgeKind(String),
}

impl std::convert::From<std::num::ParseIntError> for ConfigError {
    fn from(e: std::num::ParseIntError) -> Self {
        ConfigError::NumberParseError(e)
    }
}

fn get_base(value: &str) -> (&str, u32) {
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

fn parse_u16(value: &str) -> Result<u16, ParseIntError> {
    let (value, base) = get_base(value);
    u16::from_str_radix(value, base)
}

fn parse_u32(value: &str) -> Result<u32, ParseIntError> {
    let (value, base) = get_base(value);
    u32::from_str_radix(value, base)
}

fn parse_config(matches: ArgMatches) -> Result<Config, ConfigError> {
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

    let bridge_kind = match matches.value_of("server-kind") {
        None => BridgeKind::None,
        Some(k) => match k {
            "gdb" => BridgeKind::GDB,
            "wishbone" => BridgeKind::Wishbone,
            unknown => return Err(ConfigError::UnknownBridgeKind(unknown.to_owned())),
        },
    };

    Ok(Config {
        usb_pid,
        usb_vid,
        memory_address,
        memory_value,
        bridge_kind,
        bind_port,
        bind_addr,
    })
}

fn device_matches(cfg: &Config, device_desc: &libusb::DeviceDescriptor) -> bool {
    if let Some(pid) = cfg.usb_pid {
        if pid != device_desc.product_id() {
            return false;
        }
    }
    if let Some(vid) = cfg.usb_vid {
        if vid != device_desc.vendor_id() {
            return false;
        }
    }
    true
}

fn poke(usb: &libusb::DeviceHandle, addr: u32, value: u32) -> Result<u32, BridgeError> {
    let mut data_val = [0; 4];
    data_val[0] = ((value >> 0) & 0xff) as u8;
    data_val[1] = ((value >> 8) & 0xff) as u8;
    data_val[2] = ((value >> 16) & 0xff) as u8;
    data_val[3] = ((value >> 24) & 0xff) as u8;
    let result = usb.write_control(
        0x43,
        0,
        ((addr >> 0) & 0xffff) as u16,
        ((addr >> 16) & 0xffff) as u16,
        &data_val,
        Duration::from_millis(500),
    );
    match result {
        Err(e) => Err(BridgeError::USBError(e)),
        Ok(len) => {
            if len != 4 {
                Err(BridgeError::LengthError(4, len))
            } else {
                Ok(((data_val[3] as u32) << 24)
                    | ((data_val[2] as u32) << 16)
                    | ((data_val[1] as u32) << 8)
                    | ((data_val[0] as u32) << 0))
            }
        }
    }
}

fn peek(usb: &libusb::DeviceHandle, addr: u32) -> Result<u32, BridgeError> {
    let mut data_val = [0; 4];
    let result = usb.read_control(
        0xc3,
        0,
        ((addr >> 0) & 0xffff) as u16,
        ((addr >> 16) & 0xffff) as u16,
        &mut data_val,
        Duration::from_millis(500),
    );
    match result {
        Err(e) => Err(BridgeError::USBError(e)),
        Ok(len) => {
            if len != 4 {
                Err(BridgeError::LengthError(4, len))
            } else {
                Ok(((data_val[3] as u32) << 24)
                    | ((data_val[2] as u32) << 16)
                    | ((data_val[1] as u32) << 8)
                    | ((data_val[0] as u32) << 0))
            }
        }
    }
}

fn wishbone_server(cfg: &Config, usb: &libusb::DeviceHandle) {}

fn main() {
    let matches = App::new("Wishbone USB Adapter")
        .version("1.0")
        .author("Sean Cross <sean@xobs.io>")
        .about("Bridge Wishbone over USB")
        .arg(
            Arg::with_name("pid")
                .short("p")
                .long("pid")
                .value_name("USB_PID")
                .help("USB PID to match")
                .required_unless("vid")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("vid")
                .short("v")
                .long("vid")
                .value_name("USB_VID")
                .help("USB VID to match")
                .required_unless("pid")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("address")
                .index(1)
                .required(false)
                .help("address to read/write"),
        )
        .arg(
            Arg::with_name("value")
                .index(2)
                .required(false)
                .help("value to write"),
        )
        .arg(
            Arg::with_name("bind-addr")
                .short("a")
                .long("bind-addr")
                .value_name("IP_ADDRESS")
                .help("IP address to bind to")
                .default_value("0.0.0.0")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("port")
                .short("n")
                .long("port")
                .value_name("PORT_NUMBER")
                .help("Port number to listen on")
                .default_value("1234")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("kind")
                .short("k")
                .long("server-kind")
                .possible_values(&["gdb", "wishbone"])
                .default_value("wishbone"),
        )
        .get_matches();

    let context = libusb::Context::new().unwrap();
    let mut wb_bridge = WishboneBridge {
        // usb_ctx: libusb::Context::new().unwrap(),
        usb: None,
    };

    let cfg = parse_config(matches).unwrap();
    let mut gdb = gdb::GdbServer::new(&cfg).unwrap();
    loop {
        gdb.process().unwrap();
    }

    // loop {
    for device in context.devices().unwrap().iter() {
        let device_desc = device.device_descriptor().unwrap();
        if device_matches(&cfg, &device_desc) {
            println!(
                "Opening device {:03} on bus {:03}",
                device.bus_number(),
                device.address()
            );
            let usb = device.open().unwrap();

            if let Some(addr) = cfg.memory_address {
                if let Some(value) = cfg.memory_value {
                    poke(&usb, addr, value).unwrap();
                // println!(
                //     "Value at {:08x} is now: {:02x}{:02x}{:02x}{:02x}",
                //     addr, data_val[3], data_val[2], data_val[1], data_val[0]
                // );
                } else {
                    let val = peek(&usb, addr).unwrap();
                    println!("Value at {:08x}: {:08x}", addr, val);
                }
            }

            match cfg.bridge_kind {
                BridgeKind::None => (),
                BridgeKind::GDB => {
                    let server = gdb::GdbServer::new(&cfg);
                    loop {
                        gdb.process().unwrap();
                    }
                },
                BridgeKind::Wishbone => wishbone_server(&cfg, &usb),
            }
        }
    }
}

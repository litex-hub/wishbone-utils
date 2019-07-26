#[macro_use]
extern crate bitflags;
extern crate clap;
extern crate libusb;
extern crate rand;

extern crate flexi_logger;
// extern crate pretty_env_logger;
extern crate log;
use log::{error, info};

mod bridge;
mod config;
mod gdb;
mod riscv;
mod server;
mod usb_bridge;
mod uart_bridge;
mod utils;
mod wishbone;

use bridge::Bridge;
use server::ServerKind;
use clap::{App, Arg};
use config::Config;

use rand::prelude::*;
use riscv::RiscvCpu;

use std::thread;
use std::time::Duration;
use std::net::TcpListener;
use std::io;

#[derive(Debug)]
enum ServerError {
    IoError(io::Error),
    WishboneError(wishbone::WishboneServerError),
    GdbError(gdb::GdbServerError),
    BridgeError(bridge::BridgeError),
    RiscvCpuError(riscv::RiscvCpuError),
}
impl std::convert::From<io::Error> for ServerError {
    fn from(e: io::Error) -> ServerError {
        ServerError::IoError(e)
    }
}
impl std::convert::From<wishbone::WishboneServerError> for ServerError {
    fn from(e: wishbone::WishboneServerError) -> ServerError {
        ServerError::WishboneError(e)
    }
}
impl std::convert::From<gdb::GdbServerError> for ServerError {
    fn from(e: gdb::GdbServerError) -> ServerError {
        ServerError::GdbError(e)
    }
}
impl std::convert::From<bridge::BridgeError> for ServerError {
    fn from(e: bridge::BridgeError) -> ServerError {
        ServerError::BridgeError(e)
    }
}
impl std::convert::From<riscv::RiscvCpuError> for ServerError {
    fn from(e: riscv::RiscvCpuError) -> ServerError {
        ServerError::RiscvCpuError(e)
    }
}

fn list_usb() -> Result<(), libusb::Error> {
    let usb_ctx = libusb::Context::new().unwrap();
    let devices = usb_ctx.devices().unwrap();
    println!("devices:");
    for device in devices.iter() {
        let device_desc = device.device_descriptor().unwrap();
        let mut line = format!(
            "[{:04x}:{:04x}] - ",
            device_desc.vendor_id(),
            device_desc.product_id()
        );
        if let Ok(usb) = device.open() {
            if let Ok(langs) = usb.read_languages(Duration::from_secs(1)) {
                let product =
                    match usb.read_product_string(langs[0], &device_desc, Duration::from_secs(1)) {
                        Ok(s) => s,
                        Err(_) => "(unknown product)".to_owned(),
                    };
                let manufacturer = match usb.read_manufacturer_string(
                    langs[0],
                    &device_desc,
                    Duration::from_secs(1),
                ) {
                    Ok(s) => s,
                    Err(_) => "(unknown manufacturer)".to_owned(),
                };
                line.push_str(&format!("{} - {}", product, manufacturer));
            } else {
                line.push_str("(no strings found)");
            }
        } else {
            line.push_str("(couldn't open device)");
        }
        println!("    {}", line);
    }
    Ok(())
}

fn gdb_server(cfg: Config, bridge: Bridge) -> Result<(), ServerError> {
    let cpu = RiscvCpu::new()?;
    loop {
        let connection = {
            let listener = match TcpListener::bind(format!("{}:{}", cfg.bind_addr, cfg.bind_port)){
                Ok(o) => o,
                Err(e) => { error!("couldn't bind to address: {:?}", e); return Err(ServerError::IoError(e));},
            };

            // accept connections and process them serially
            info!(
                "accepting connections on {}:{}",
                cfg.bind_addr, cfg.bind_port
            );
            let (connection, _sockaddr) = match listener.accept() {
                Ok(o) => o,
                Err(e) => {error!("couldn't accept connection: {:?}", e); return Err(ServerError::IoError(e));},
            };
            let peer_addr = match connection.peer_addr() {
                Ok(o) => o,
                Err(e) => {error!("couldn't get remote address: {:?}", e); return Err(ServerError::IoError(e)); },
            };
            info!("connection from {}", peer_addr);
            connection
        };

        let mut gdb = gdb::GdbServer::new(connection).unwrap();
        let cpu_controller = cpu.get_controller();
        let mut gdb_controller = gdb.get_controller();
        if let Err(e) = cpu.halt(&bridge) {
            error!("couldn't halt CPU: {:?}", e);
            continue;
        }

        let poll_bridge = bridge.clone();
        thread::spawn(move || loop {
            let mut had_error = false;
            loop {
                if let Err(e) = cpu_controller.poll(&poll_bridge, &mut gdb_controller) {
                    if ! had_error {
                        error!("error while polling bridge: {:?}", e);
                        had_error = true;
                    }
                } else {
                    had_error = false;
                }
                thread::park_timeout(Duration::from_millis(200));
            }
        });

        loop {
            let cmd = match gdb.get_command() {
                Err(e) => {
                    error!("unable to read command from GDB client: {:?}", e);
                    break;
                }
                Ok(o) => o
            };

            if let Err(e) = gdb.process(cmd, &cpu, &bridge) {
                match e {
                    gdb::GdbServerError::ConnectionClosed => (),
                    e => error!("error in GDB server: {:?}", e),
                }
                break;
            }
        }
    }
}

fn wishbone_server(cfg: Config, bridge: Bridge) -> Result<(), ServerError> {
    let mut wishbone = wishbone::WishboneServer::new(&cfg).unwrap();
    loop {
        if let Err(e) = wishbone.connect() {
            error!("Unable to connect to Wishbone bridge: {:?}", e);
            return Err(ServerError::WishboneError(e));
        }
        loop {
            if let Err(e) = wishbone.process(&bridge) {
                println!("Error in Wishbone server: {:?}", e);
                break;
            }
        }
    }
}

fn random_test(cfg: Config, bridge: Bridge) -> Result<(), ServerError> {
    let mut loop_counter: u32 = 0;
    loop {
        let random_addr = 0x10000000 + 8192;
        let val = random::<u32>();
        bridge.poke(random_addr, val).unwrap();
        let cmp = bridge.peek(random_addr).unwrap();
        if cmp != val {
            panic!(
                "Loop {}: Expected {:08x}, got {:08x}",
                loop_counter, val, cmp
            );
        }
        if (loop_counter % 1000) == 0 {
            println!("loop: {} ({:08x})", loop_counter, val);
        }
        loop_counter = loop_counter.wrapping_add(1);
        if let Some(max_loops) = cfg.random_loops {
            if loop_counter > max_loops {
                println!("No errors encountered");
                return Ok(());
            }
        }
    }
}

fn memory_access(cfg: Config, bridge: Bridge) -> Result<(), ServerError> {
    if let Some(addr) = cfg.memory_address {
        if let Some(value) = cfg.memory_value {
            bridge.poke(addr, value)?;
        } else {
            let val = bridge.peek(addr)?;
            println!("Value at {:08x}: {:08x}", addr, val);
        }
    } else {
        println!("No operation and no address specified!");
        println!("Try specifying an address such as \"0x10000000\".  See --help for more information");
    }
    Ok(())
}

fn main() {
    flexi_logger::Logger::with_env_or_str("wishbone_tool=info").start().unwrap();
    // pretty_env_logger::init();
    let matches = App::new("Wishbone USB Adapter")
        .version("1.0")
        .author("Sean Cross <sean@xobs.io>")
        .about("Bridge Wishbone over USB")
        .arg(
            Arg::with_name("list")
                .short("l")
                .long("list")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("pid")
                .short("p")
                .long("pid")
                .value_name("USB_PID")
                .help("USB PID to match")
                .default_value("0x5bf0")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("vid")
                .short("v")
                .long("vid")
                .value_name("USB_VID")
                .help("USB VID to match")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("serial")
                .short("u")
                .long("serial")
                .alias("uart")
                .value_name("PORT")
                .help("Serial port to use")
                .takes_value(true)
        )
        .arg(
            Arg::with_name("baud")
                .short("b")
                .long("baud")
                .value_name("RATE")
                .default_value("115200")
                .help("Baudrate to use in serial mode")
                .takes_value(true)
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
                .help("port number to listen on")
                .default_value("1234")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("bridge-kind")
                .short("s")
                .long("server-kind")
                .takes_value(true)
                .help("which server to run (if any)")
                .possible_values(&["gdb", "wishbone", "random-test"]),
        )
        .arg(
            Arg::with_name("random-loops")
                .long("random-loops")
                .help("number of loops to run when doing a random-test")
                .takes_value(true),
        )
        .get_matches();

    if matches.is_present("list") {
        if list_usb().is_err() {
            println!("USB is not properly configured");
        };
        return;
    }

    let cfg = Config::parse(matches).unwrap();

    let bridge = Bridge::new(&cfg).unwrap();
    bridge.connect().unwrap();

    let retcode = match cfg.server_kind {
        ServerKind::GDB => gdb_server(cfg, bridge),
        ServerKind::Wishbone => wishbone_server(cfg, bridge),
        ServerKind::RandomTest => random_test(cfg, bridge),
        ServerKind::None => memory_access(cfg, bridge),
    };
    if let Err(e) = retcode {
        error!("Unsuccessful exit: {:?}", e);
    }
}

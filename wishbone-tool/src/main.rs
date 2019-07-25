#[macro_use]
extern crate bitflags;
extern crate clap;
extern crate libusb;
extern crate rand;

extern crate flexi_logger;
// extern crate pretty_env_logger;
extern crate log;
use log::{debug, error, info};

mod bridge;
mod config;
mod gdb;
mod riscv;
mod usb_bridge;
mod utils;
mod wishbone;

use bridge::{Bridge, BridgeKind};
use clap::{App, Arg};
use config::Config;

use rand::prelude::*;
use riscv::RiscvCpu;

use std::thread;
use std::time::Duration;
use std::net::TcpListener;

fn list_usb() -> Result<(), libusb::Error> {
    let usb_ctx = libusb::Context::new().unwrap();
    let devices = usb_ctx.devices().unwrap();
    println!("Devices:");
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

    let cpu = RiscvCpu::new().unwrap();
    let cfg = Config::parse(matches).unwrap();

    let bridge = Bridge::new(&cfg).unwrap();
    bridge.connect().unwrap();

    match cfg.bridge_kind {
        BridgeKind::GDB => loop {
            let connection = {
                let listener = TcpListener::bind(format!("{}:{}", cfg.bind_addr, cfg.bind_port)).expect("Couldn't bind to address");

                // accept connections and process them serially
                info!(
                    "Accepting connections on {}:{}",
                    cfg.bind_addr, cfg.bind_port
                );
                let (connection, _sockaddr) = listener.accept().expect("Couldn't accept connection");
                info!("Connection from {:?}", connection.peer_addr().expect("Couldn't get remote address"));
                connection
            };

            let mut gdb = gdb::GdbServer::new(connection).unwrap();
            let cpu_controller = cpu.get_controller();
            let mut gdb_controller = gdb.get_controller();
            cpu.halt(&bridge).expect("Couldn't halt CPU");
            let poll_bridge = bridge.clone();
            thread::spawn(move || loop {
                if let Err(e) = cpu_controller.poll(&poll_bridge, &mut gdb_controller) {
                    error!("Error while polling bridge: {:?}", e);
                    return;
                }
                thread::park_timeout(Duration::from_millis(200));
            });
            loop {
                let cmd = match gdb.get_command() {
                    Err(e) => {
                        error!("Unable to read command from GDB client: {:?}", e);
                        break;
                    }
                    Ok(o) => o
                };

                if let Err(e) = gdb.process(cmd, &cpu, &bridge) {
                    match e {
                        gdb::GdbServerError::ConnectionClosed => (),
                        e => error!("Error in GDB server: {:?}", e),
                    }
                    break;
                }
            }
        },
        BridgeKind::Wishbone => {
            let mut wishbone = wishbone::WishboneServer::new(&cfg).unwrap();
            loop {
                wishbone.connect().unwrap();
                loop {
                    if let Err(e) = wishbone.process(&bridge) {
                        println!("Error in Wishbone server: {:?}", e);
                        break;
                    }
                }
            }
        }
        BridgeKind::RandomTest => {
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
                        return;
                    }
                }
            }
        }
        BridgeKind::None => {
            if let Some(addr) = cfg.memory_address {
                if let Some(value) = cfg.memory_value {
                    bridge.poke(addr, value).unwrap();
                } else {
                    let val = bridge.peek(addr).unwrap();
                    println!("Value at {:08x}: {:08x}", addr, val);
                }
            } else {
                println!("No operation and no address specified!");
                println!("Try specifying an address such as \"0x10000000\".  See --help for more information");
            }
        }
    }
}

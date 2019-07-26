#[macro_use]
extern crate bitflags;
extern crate clap;
extern crate libusb;
extern crate rand;

extern crate flexi_logger;
// extern crate pretty_env_logger;
extern crate log;
use log::error;

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

use std::time::Duration;

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
        ServerKind::GDB => server::gdb_server(cfg, bridge),
        ServerKind::Wishbone => server::wishbone_server(cfg, bridge),
        ServerKind::RandomTest => server::random_test(cfg, bridge),
        ServerKind::None => server::memory_access(cfg, bridge),
    };
    if let Err(e) = retcode {
        error!("Unsuccessful exit: {:?}", e);
    }
}

extern crate clap;
extern crate libusb;

mod bridge;
mod config;
mod gdb;
mod utils;
mod usb_bridge;

use clap::{App, Arg};
use config::Config;

use bridge::BridgeKind;
use usb_bridge::UsbBridge;

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

    let cfg = Config::parse(matches).unwrap();
    let mut gdb = gdb::GdbServer::new(&cfg).unwrap();
    let mut usb_bridge = UsbBridge::new(&cfg).unwrap();

/*
            if let Some(addr) = cfg.memory_address {
                if let Some(value) = cfg.memory_value {
                    usb_bridge.poke(addr, value).unwrap();
                } else {
                    let val = usb_bridge.peek(addr).unwrap();
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
                }
                BridgeKind::Wishbone => wishbone_server(&cfg, &usb),
            }
            */
}

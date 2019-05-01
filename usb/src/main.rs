extern crate clap;
extern crate libusb;

use clap::{App, Arg};
use std::num::ParseIntError;
use std::time::Duration;

struct WishboneBridge<'a> {
    // usb_ctx: libusb::Context,
    usb: Option<libusb::DeviceHandle<'a>>,
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
                .required(true)
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
        .get_matches();

    let context = libusb::Context::new().unwrap();
    let mut wb_bridge = WishboneBridge {
        // usb_ctx: libusb::Context::new().unwrap(),
        usb: None,
    };

    let usb_vid = if let Some(vid) = matches.value_of("vid") {
        parse_u16(vid).unwrap()
    } else {
        0
    };

    let usb_pid = if let Some(pid) = matches.value_of("pid") {
        parse_u16(pid).unwrap()
    } else {
        0
    };

    let addr = if let Some(addr) = matches.value_of("address") {
        parse_u32(addr).unwrap()
    } else {
        0
    };

    // loop {
    for device in context.devices().unwrap().iter() {
        let device_desc = device.device_descriptor().unwrap();
        if (usb_pid == 0 || usb_pid == device_desc.product_id())
            && (usb_vid == 0 || usb_vid == device_desc.vendor_id())
        {
            println!(
                "Opening device {:03} on bus {:03}",
                device.bus_number(),
                device.address()
            );
            let mut data_val = [0; 4];
            let usb = device.open().unwrap();
            if let Some(value_str) = matches.value_of("value") {
                let value = parse_u32(value_str).unwrap();
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
                    Err(e) => panic!("unable to write: {:?}", e),
                    Ok(len) => {
                        if len != 4 {
                            panic!("expected 4 bytes, got {} bytes", len);
                        } else {
                            println!(
                                "Value at {:08x} is now: {:02x}{:02x}{:02x}{:02x}",
                                addr, data_val[3], data_val[2], data_val[1], data_val[0]
                            );
                        }
                    }
                }
            } else {
                let result = usb.read_control(
                    0xc3,
                    0,
                    ((addr >> 0) & 0xffff) as u16,
                    ((addr >> 16) & 0xffff) as u16,
                    &mut data_val,
                    Duration::from_millis(500),
                );
                match result {
                    Err(e) => panic!("unable to read: {:?}", e),
                    Ok(len) => {
                        if len != 4 {
                            panic!("expected 4 bytes, got {} bytes", len);
                        } else {
                            println!(
                                "Value at {:08x}: {:02x}{:02x}{:02x}{:02x}",
                                addr, data_val[3], data_val[2], data_val[1], data_val[0]
                            );
                        }
                    }
                }
            }
        }
        // println!(
        //     "Bus {:03} Device {:03} ID {:04x}:{:04x}",
        //     device.bus_number(),
        //     device.address(),
        //     device_desc.vendor_id(),
        //     device_desc.product_id()
        // );
    }
    // }
}

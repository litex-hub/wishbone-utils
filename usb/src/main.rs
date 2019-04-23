extern crate clap;
extern crate libusb;

use clap::{App, Arg};

struct WishboneBridge<'a> {
    // usb_ctx: libusb::Context,
    usb: Option<libusb::DeviceHandle<'a>>,
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
        vid.parse::<u16>().unwrap()
    } else {
        0
    };

    let usb_pid = if let Some(pid) = matches.value_of("pid") {
        pid.parse::<u16>().unwrap()
    } else {
        0
    };

    loop {
        for device in context.devices().unwrap().iter() {
            let device_desc = device.device_descriptor().unwrap();
            if (usb_pid == 0 || usb_pid == device_desc.product_id())
            && (usb_vid == 0 || usb_vid == device_desc.vendor_id()) {
                println!("Opening device {:03} on bus {:03}", device.bus_number(), device.address());
                wb_bridge.usb = Some(device.open().unwrap());
            }
            // println!(
            //     "Bus {:03} Device {:03} ID {:04x}:{:04x}",
            //     device.bus_number(),
            //     device.address(),
            //     device_desc.vendor_id(),
            //     device_desc.product_id()
            // );
        }
    }
}

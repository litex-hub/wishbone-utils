#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate clap;
extern crate csv;
extern crate terminal;
extern crate libusb;
extern crate rand;

extern crate flexi_logger;
extern crate log;
use log::error;

mod bridge;
mod config;
mod gdb;
mod riscv;
mod server;
mod wishbone;

use bridge::Bridge;

use clap::{App, Arg, Shell};
use config::Config;
use server::ServerKind;

use std::process;
use std::time::Duration;

fn list_usb() -> Result<(), libusb::Error> {
    let usb_ctx = libusb::Context::new().unwrap();
    let devices = usb_ctx.devices().unwrap();
    println!("devices:");
    for device in devices.iter() {
        let device_desc = device.device_descriptor().unwrap();
        let usb_bus = device.bus_number();
        let usb_device = device.address();
        let mut line = format!(
            "[{:04x}:{:04x}] usb: {:03}/{:03} - ",
            device_desc.vendor_id(),
            device_desc.product_id(),
            usb_bus,
            usb_device,
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

fn clap_app<'a, 'b>() -> App<'a, 'b> {
    App::new("Wishbone Tool")
        .version(crate_version!())
        .author("Sean Cross <sean@xobs.io>")
        .about("Work with Wishbone devices over various bridges")
        .arg(
            Arg::with_name("list")
                .short("l")
                .long("list")
                .help("List USB devices in the system")
                .required_unless("completion")
                .conflicts_with("completion")
                .required_unless("address")
                .conflicts_with("address")
                .required_unless("server-kind")
                .conflicts_with("server-kind")
                .display_order(3)
                .takes_value(false),
        )
        .arg(
            Arg::with_name("completion")
                .short("c")
                .long("completion")
                .help("Generate shell auto-completion file")
                .required_unless("list")
                .conflicts_with("list")
                .required_unless("address")
                .conflicts_with("address")
                .required_unless("server-kind")
                .conflicts_with("server-kind")
                .display_order(3)
                .possible_values(&Shell::variants())
                .takes_value(true)
        )
        .arg(
            Arg::with_name("pid")
                .short("p")
                .long("pid")
                .value_name("USB_PID")
                .help("USB PID to match")
                .default_value("0x5bf0")
                .display_order(3)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("vid")
                .short("v")
                .long("vid")
                .value_name("USB_VID")
                .help("USB VID to match")
                .display_order(3)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("bus")
                .short("B")
                .long("bus")
                .value_name("USB_BUS")
                .help("USB BUS to match")
                .display_order(4)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("device")
                .short("d")
                .long("device")
                .value_name("USB_DEVICE")
                .help("USB DEVICE to match")
                .display_order(4)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("serial")
                .short("u")
                .long("serial")
                .alias("uart")
                .value_name("PORT")
                .help("Serial port to use")
                .display_order(4)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("baud")
                .short("b")
                .long("baud")
                .value_name("RATE")
                .default_value("115200")
                .help("Baudrate to use in serial mode")
                .display_order(5)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("spi-pins")
                .short("g")
                .long("spi-pins")
                .value_delimiter("PINS")
                .help("GPIO pins to use for MISO,MOSI,CLK,CS_N (e.g. 2,3,4,18)")
                .display_order(6)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("address")
                .index(1)
                .required_unless("completion")
                .conflicts_with("completion")
                .required_unless("server-kind")
                .conflicts_with("server-kind")
                .required_unless("list")
                .conflicts_with("list")
                .display_order(7)
                .help("address to read/write"),
        )
        .arg(
            Arg::with_name("value")
                .value_name("value")
                .index(2)
                .required(false)
                .display_order(8)
                .help("value to write"),
        )
        .arg(
            Arg::with_name("bind-addr")
                .short("a")
                .long("bind-addr")
                .value_name("IP_ADDRESS")
                .help("IP address to bind to")
                .default_value("127.0.0.1")
                .display_order(2)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("port")
                .short("n")
                .long("port")
                .value_name("PORT_NUMBER")
                .help("port number to listen on")
                .default_value("1234")
                .display_order(2)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("server-kind")
                .short("s")
                .long("server")
                .alias("server-kind")
                .takes_value(true)
                .multiple(true)
                .required_unless("completion")
                .conflicts_with("completion")
                .required_unless("address")
                .conflicts_with("address")
                .required_unless("list")
                .conflicts_with("list")
                .help("which server to run (if any)")
                .display_order(1)
                .possible_values(&["gdb", "wishbone", "random-test", "load-file", "terminal"]),
        )
        .arg(
            Arg::with_name("load-name")
                .long("load-name")
                .help("A file to load into RAM")
                .takes_value(true)
                .display_order(13),
        )
        .arg(
            Arg::with_name("load-address")
                .long("load-address")
                .help("Address for file to load")
                .takes_value(true)
                .display_order(13),
        )
        .arg(
            Arg::with_name("random-loops")
                .long("random-loops")
                .help("number of loops to run when doing a random-test")
                .display_order(9)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("random-range")
                .long("random-range")
                .help("the size of the random address range (i.e. how many bytes to randomly add to the address)")
                .display_order(9)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("messible-address")
                .long("messible-address")
                .help("address to use to get messible messages from")
                .display_order(9)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("csr-csv")
                .long("csr-csv")
                .help("csr.csv file containing register mappings")
                .display_order(9)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("random-address")
                .long("random-address")
                .help("address to write to when doing a random-test")
                .display_order(10)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("debug-offset")
                .long("debug-offset")
                .help("address to use for debug bridge")
                .default_value("0xf00f0000")
                .display_order(11)
                .takes_value(true),
        )
}

fn main() {
    flexi_logger::Logger::with_env_or_str("wishbone_tool=info")
        .start()
        .unwrap();
    let matches = clap_app().get_matches();

    if matches.is_present("list") {
        if list_usb().is_err() {
            println!("USB is not properly configured");
        };
        return;
    }

    if let Some(shell_str) = matches.value_of("completion") {
        use std::io;
        use std::str::FromStr;
        let shell = match Shell::from_str(shell_str) {
            Ok(s) => s,
            Err(_) => panic!("Unrecognized shell"),
        };
        clap_app().gen_completions_to(
            "wishbone-tool",
            shell,
            &mut io::stdout()
        );
        return;
    }

    let cfg = match Config::parse(matches) {
        Ok(cfg) => cfg,
        Err(e) => {
            match e {
                config::ConfigError::NumberParseError(num, e) => {
                    error!("unable to parse the number \"{}\": {}", num, e)
                }
                config::ConfigError::NoOperationSpecified => panic!("no operation was specified"),
                config::ConfigError::UnknownServerKind(s) => {
                    error!("unknown server '{}', see --help", s)
                }
                config::ConfigError::SpiParseError(s) => error!("couldn't parse spi pins: {}", s),
            }
            process::exit(1);
        }
    };

    {
        let bridge = Bridge::new(&cfg).unwrap();
        bridge.connect().unwrap();
        let mut threads = vec![];
        for server_kind in &cfg.server_kind {
            use std::thread;
            let bridge = bridge.clone();
            let cfg = cfg.clone();
            let kind = server_kind.clone();
            let thr_handle = thread::spawn(move || {
                match kind {
                    ServerKind::GDB => server::gdb_server(cfg, bridge),
                    ServerKind::Wishbone => server::wishbone_server(cfg, bridge),
                    ServerKind::RandomTest => server::random_test(cfg, bridge),
                    ServerKind::LoadFile => server::load_file(cfg, bridge),
                    ServerKind::Terminal => server::terminal_client(cfg, bridge),
                    ServerKind::MemoryAccess => server::memory_access(cfg, bridge),
                }
            });
            threads.push(thr_handle);
        }
        for handle in threads {
            handle.join().ok();
        }
    };
    // if let Err(e) = retcode {
    //     error!("server error: {:?}", e);
    //     process::exit(1);
    // }
}

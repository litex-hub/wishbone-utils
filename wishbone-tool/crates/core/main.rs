#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate clap;

extern crate indicatif;

use log::debug;

mod config;
mod gdb;
mod riscv;
mod server;
mod wishbone;

use clap::{App, Arg, Shell};
use config::Config;
use server::ServerKind;

use std::sync::Arc;

fn clap_app<'a, 'b>() -> App<'a, 'b> {
    App::new("Wishbone Tool")
        .version(crate_version!())
        .author("Sean Cross <sean@xobs.io>")
        .about("Work with Wishbone devices over various bridges")
        .arg(
            Arg::with_name("completion")
            .group("command")
                .short("c")
                .long("completion")
                .help("Generate shell auto-completion file")
                .display_order(1)
                .possible_values(&Shell::variants())
                .takes_value(true)
        )

        .arg(
            Arg::with_name("pid")
                .short("p")
                .long("pid")
                .value_name("USB_PID")
                .help("USB: PID to match")
                .default_value("0x5bf0")
                .display_order(2)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("vid")
                .short("v")
                .long("vid")
                .value_name("USB_VID")
                .help("USB: VID to match")
                .display_order(2)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("bus")
                .short("B")
                .long("bus")
                .value_name("USB_BUS")
                .help("USB: bus to match")
                .display_order(3)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("device")
                .short("d")
                .long("device")
                .value_name("USB_DEVICE")
                .help("USB: device to match")
                .display_order(3)
                .takes_value(true),
        )

        .arg(
            Arg::with_name("serial")
                .short("u")
                .long("serial")
                .alias("uart")
                .value_name("PORT")
                .help("SERIAL: path to serial port")
                .display_order(4)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("baud")
                .short("b")
                .long("baud")
                .value_name("RATE")
                .default_value("115200")
                .help("SERIAL: baudrate to use for serial port")
                .display_order(5)
                .takes_value(true),
        )

        .arg(
            Arg::with_name("ethernet-host")
                .long("ethernet-host")
                .value_name("ADDRESS")
                .help("ETHERNET: address of device or proxy to connect to")
                .display_order(6)
                .takes_value(true)
        )
        .arg(
            Arg::with_name("ethernet-port")
                .long("ethernet-port")
                .value_name("PORT")
                .help("ETHERNET: port to use for Ethernet bridge")
                .default_value("1234")
                .display_order(7)
                .takes_value(true)
        )
        .arg(
            Arg::with_name("ethernet-tcp")
                .long("ethernet-tcp")
                .help("ETHERNET: use TCP to connect to Wishbone, such as when using a proxy")
                .display_order(8)
        )

        .arg(
            Arg::with_name("pcie-bar")
                .long("pcie-bar")
                .help("PCIe: use the specified file as a PCIe BAR")
                .display_order(9)
                .takes_value(true)
        )

        .arg(
            Arg::with_name("spi-pins")
                .short("g")
                .long("spi-pins")
                .value_delimiter("PINS")
                .help("SPI: GPIO pins to use for COPI,CIPO,CLK,CS_N (e.g. 2,3,4,18)")
                .display_order(10)
                .takes_value(true),
        )

        .arg(
            Arg::with_name("address")
                .index(1)
                .group("command")
                .display_order(11)
                .help("address to read/write"),
        )
        .arg(
            Arg::with_name("value")
                .value_name("value")
                .index(2)
                .required(false)
                .display_order(12)
                .help("value to write"),
        )

        .arg(
            Arg::with_name("csr-csv")
                .long("csr-csv")
                .help("csr.csv file containing register mappings")
                .display_order(13)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("register-offset")
                .long("register-offset")
                .alias("csr-csv-offset")
                .help("apply an offset to addresses, e.g. to compensate for PCIe BAR offset")
                .display_order(14)
                .takes_value(true),
        )

        .arg(
            Arg::with_name("server-kind")
                .short("s")
                .group("command")
                .long("server")
                .alias("server-kind")
                .takes_value(true)
                .multiple(true)
                .help("which server to run (if any)")
                .display_order(15)
                .possible_values(&["gdb", "wishbone", "random-test", "load-file", "terminal", "messible"]),
        )

        .arg(
            Arg::with_name("gdb-port")
                .long("gdb-port")
                .help("GDB: port to listen on for GDB connections")
                .default_value("3333")
                .display_order(16)
                .takes_value(true)
        )
        .arg(
            Arg::with_name("debug-offset")
                .long("debug-offset")
                .help("GDB: address of the CPU's debug bridge")
                .default_value("0xf00f0000")
                .display_order(17)
                .takes_value(true),
        )

        .arg(
            Arg::with_name("bind-addr")
                .short("a")
                .long("bind-addr")
                .value_name("IP_ADDRESS")
                .help("WISHBONE: IP address to bind to when acting as a server")
                .default_value("127.0.0.1")
                .display_order(18)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("wishbone-port")
                .short("n")
                .long("wishbone-port")
                .alias("port")
                .value_name("PORT_NUMBER")
                .help("WISHBONE: port number to listen on when acting as a server")
                .default_value("1234")
                .display_order(19)
                .takes_value(true),
        )

        .arg(
            Arg::with_name("random-address")
                .long("random-address")
                .help("RANDOM_TEST: address at which to write")
                .display_order(20)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("random-loops")
                .long("random-loops")
                .help("RANDOM_TEST: Number of loops to run")
                .display_order(21)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("random-range")
                .long("random-range")
                .help("RANDOM_TEST: the size of the random address range (i.e. how many bytes to randomly add to the address)")
                .display_order(22)
                .takes_value(true),
        )

        .arg(
            Arg::with_name("load-name")
                .long("load-name")
                .help("LOAD_FILE: Name of the file to load into RAM or FLASH (defaults to RAM unless load-flash is set)")
                .takes_value(true)
                .display_order(23),
        )
        .arg(
            Arg::with_name("load-address")
                .long("load-address")
                .help("LOAD_FILE: Address at which to load the file")
                .takes_value(true)
                .display_order(24),
        )

        .arg(
            Arg::with_name("load-flash")
                 .long("load-flash")
                 .help("when specified, load-name and load-address attempt to load to FLASH")
                 .display_order(25),
        )

        .arg(
            Arg::with_name("terminal-mouse")
                .long("terminal-mouse")
                .help("TERMINAL: enable capturing of mouse events")
                .display_order(26)
                .takes_value(false)
        )

        .arg(
            Arg::with_name("messible-address")
                .long("messible-address")
                .help("MESSIBLE: address to use to get messible messages from")
                .display_order(27)
                .takes_value(true),
        )

        .arg(
            Arg::with_name("burst-length")
            .long("burst-length")
            .help("Number of bytes in a burst (implies burst operation)")
            .default_value("4")
            .display_order(28)
            .takes_value(true),
        )

        .arg(
            Arg::with_name("hexdump")
            .long("hexdump")
            .help("In conjunction with burst-length, report reads as text hexdumps, instead of binary data")
            .display_order(29)
            .takes_value(false),
        )

        .arg(
            Arg::with_name("burst-source")
            .long("burst-source")
            .help("File for burst data input when sending data to device")
            .display_order(30)
            .takes_value(true),
        )

        .arg(
            Arg::with_name("flash-no-reset")
            .long("flash-no-reset")
            .help("Don't reset the CPU after resuming")
            .display_order(31)
            .takes_value(false),
        )

        .arg(
            Arg::with_name("careful-flashing")
            .long("careful-flashing")
            .help("Check all intermediate results from burning, instead of just relying on post-flash readback verification. Roughly doubles programming time.")
            .display_order(32)
            .takes_value(false),
        )
}

fn main() -> Result<(), String> {
    flexi_logger::Logger::with_env_or_str("wishbone_tool=info")
        .format_for_stderr(|write, now, record| {
            flexi_logger::colored_default_format(write, now, record)?;
            write!(write, "\r")
        })
        .start()
        .unwrap();

    let matches = clap_app().get_matches();

    // If they specify a "--completion", print it to stdout and exit without error.
    if let Some(shell_str) = matches.value_of("completion") {
        use std::io;
        use std::str::FromStr;
        // Unwrap is safe since `get_matches()` validated it above
        let shell = Shell::from_str(shell_str).unwrap();
        clap_app().gen_completions_to(crate_name!(), shell, &mut io::stdout());
        return Ok(());
    }

    let (cfg, bridge) = Config::parse(matches).map_err(|e| match e {
        config::ConfigError::NumberParseError(num, e) => {
            format!("unable to parse the number \"{}\": {}", num, e)
        }
        config::ConfigError::NoOperationSpecified => format!("no operation was specified"),
        config::ConfigError::UnknownServerKind(s) => format!("unknown server '{}', see --help", s),
        config::ConfigError::SpiParseError(s) => format!("couldn't parse spi pins: {}", s),
        config::ConfigError::IoError(s) => format!("file error: {}", s),
        config::ConfigError::InvalidConfig(s) => format!("invalid configuration: {}", s),
        config::ConfigError::AddressOutOfRange(s) => {
            format!("address was not in mappable range: {}", s)
        }
    })?;
    bridge
        .connect()
        .map_err(|e| format!("unable to connect to bridge: {}", e))?;

    let cfg = Arc::new(cfg);
    let mut threads = vec![];
    for server_kind in cfg.server_kind.iter() {
        use std::thread;
        let bridge = bridge.clone();
        let cfg = cfg.clone();
        let server_kind = *server_kind;
        let thr_handle = thread::spawn(move || {
            match server_kind {
                ServerKind::GDB => server::gdb_server(&cfg, bridge),
                ServerKind::Wishbone => server::wishbone_server(&cfg, bridge),
                ServerKind::RandomTest => server::random_test(&cfg, bridge),
                ServerKind::LoadFile => server::load_file(&cfg, bridge),
                ServerKind::Terminal => server::terminal_client(&cfg, bridge),
                ServerKind::MemoryAccess => server::memory_access(&cfg, bridge),
                ServerKind::Messible => server::messible_client(&cfg, bridge),
                ServerKind::FlashProgram => server::flash_program(&cfg, bridge),
            }
            .expect("couldn't start server");
            debug!("Exited {:?} thread", server_kind);
        });
        threads.push(thr_handle);
    }
    for handle in threads {
        handle.join().ok();
    }

    Ok(())
}

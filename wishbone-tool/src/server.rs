use crate::bridge;
use crate::config::{Config, ConfigError};
use crate::gdb;
use crate::riscv;
use crate::wishbone;

extern crate log;
use log::{error, info};

extern crate rand;
use rand::prelude::*;

use std::io;
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

#[derive(PartialEq)]
pub enum ServerKind {
    /// No server
    None,

    /// Wishbone bridge
    Wishbone,

    /// GDB server
    GDB,

    /// Send random data back and forth
    RandomTest,
}

#[derive(Debug)]
pub enum ServerError {
    IoError(io::Error),
    WishboneError(wishbone::WishboneServerError),
    GdbError(gdb::GdbServerError),
    BridgeError(bridge::BridgeError),
    RiscvCpuError(riscv::RiscvCpuError),
    RandomValueError(
        u32, /* counter */
        u32, /* expected */
        u32, /* observed */
    ),
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

impl ServerKind {
    pub fn from_string(item: &Option<&str>) -> Result<ServerKind, ConfigError> {
        match item {
            None => Ok(ServerKind::None),
            Some(k) => match *k {
                "gdb" => Ok(ServerKind::GDB),
                "wishbone" => Ok(ServerKind::Wishbone),
                "random-test" => Ok(ServerKind::RandomTest),
                unknown => Err(ConfigError::UnknownServerKind(unknown.to_owned())),
            },
        }
    }
}

/// Poll the Messible at the address specified.
/// Return `true` if there is still data to be read
/// after returning.
fn poll_messible(
    messible_address: &Option<u32>,
    bridge: &bridge::Bridge,
    gdb_controller: &mut gdb::GdbController,
) -> bool {
    let addr = match messible_address {
        None => return false,
        Some(s) => s,
    };

    let mut data: Vec<u8> = vec![];
    let max_bytes = 64;
    while data.len() < max_bytes {
        let status = match bridge.peek(addr + 8) {
            Ok(b) => b,
            Err(_) => return false,
        };

        if status & 2 == 0 {
            break;
        }

        let b = match bridge.peek(addr + 4) {
            Ok(b) => b as u8,
            Err(_) => return false,
        };

        data.push(b);
    }

    let s = match std::str::from_utf8(&data) {
        Ok(o) => o,
        Err(_) => "[invalid string]",
    };
    gdb_controller.print_string(s).ok();

    // Re-examine the Messible and determine if we still have data
    match bridge.peek(addr + 8) {
        Ok(b) => (b & 2) != 0,
        Err(_) => false,
    }
}

pub fn gdb_server(cfg: Config, bridge: bridge::Bridge) -> Result<(), ServerError> {
    let cpu = riscv::RiscvCpu::new(&bridge, cfg.debug_offset)?;
    let messible_address = cfg.messible_address;
    loop {
        let connection = {
            let listener = match TcpListener::bind(format!("{}:{}", cfg.bind_addr, cfg.bind_port)) {
                Ok(o) => o,
                Err(e) => {
                    error!("couldn't bind to address: {:?}", e);
                    return Err(ServerError::IoError(e));
                }
            };

            // accept connections and process them serially
            info!(
                "accepting connections on {}:{}",
                cfg.bind_addr, cfg.bind_port
            );
            let (connection, _sockaddr) = match listener.accept() {
                Ok(o) => o,
                Err(e) => {
                    error!("couldn't accept connection: {:?}", e);
                    return Err(ServerError::IoError(e));
                }
            };
            let peer_addr = match connection.peer_addr() {
                Ok(o) => o,
                Err(e) => {
                    error!("couldn't get remote address: {:?}", e);
                    return Err(ServerError::IoError(e));
                }
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
                let mut do_pause = true;
                match cpu_controller.poll(&poll_bridge, &mut gdb_controller) {
                    Err(e) => {
                        if !had_error {
                            error!("error while polling bridge: {:?}", e);
                            had_error = true;
                        }
                    }
                    Ok(running) => {
                        had_error = false;
                        // If there's a messible available, poll it.
                        if running {
                            do_pause = ! poll_messible(&messible_address, &poll_bridge, &mut gdb_controller);
                        }
                    }
                }

                if do_pause {
                    thread::park_timeout(Duration::from_millis(200));
                }
            }
        });

        loop {
            let cmd = match gdb.get_command() {
                Err(e) => {
                    error!("unable to read command from GDB client: {:?}", e);
                    break;
                }
                Ok(o) => o,
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

pub fn wishbone_server(cfg: Config, bridge: bridge::Bridge) -> Result<(), ServerError> {
    let mut wishbone = wishbone::WishboneServer::new(&cfg).unwrap();
    let messible_address = cfg.messible_address;

    loop {
        if let Err(e) = wishbone.connect() {
            error!("Unable to connect to Wishbone bridge: {:?}", e);
            return Err(ServerError::WishboneError(e));
        }

        // If there's a messible address specified, enable printf-style debugging.
        if let Some(addr) = messible_address {
            let poll_bridge = bridge.clone();
            thread::spawn(move || loop {
                let mut data: Vec<u8> = vec![];
                let max_bytes = 64;
                while data.len() < max_bytes {
                    // Get the status to see if it's empty.
                    let status = match poll_bridge.peek(addr + 8) {
                        Ok(b) => b,
                        Err(_) => return false,
                    };

                    // If the messible is empty, stop filling the buffer.
                    if status & 2 == 0 {
                        break;
                    }

                    // It's not empty, so grab the next character
                    let b = match poll_bridge.peek(addr + 4) {
                        Ok(b) => b as u8,
                        Err(_) => return false,
                    };

                    data.push(b);
                }

                let s = match std::str::from_utf8(&data) {
                    Ok(o) => o,
                    Err(_) => "[invalid string]",
                };
                print!("{}", s);

                // Re-examine the Messible and determine if we still have data
                let do_pause = match poll_bridge.peek(addr + 8) {
                    Ok(b) => (b & 2) == 0,
                    Err(_) => return false,
                };

                if do_pause {
                    thread::park_timeout(Duration::from_millis(200));
                }
            });
        }

        loop {
            if let Err(e) = wishbone.process(&bridge) {
                println!("Error in Wishbone server: {:?}", e);
                break;
            }
        }
    }
}

pub fn random_test(cfg: Config, bridge: bridge::Bridge) -> Result<(), ServerError> {
    let mut loop_counter: u32 = 0;
    let random_addr = match cfg.random_address {
        Some(s) => s,
        None => 0x10000000 + 8192,
    };
    info!("writing random values to 0x{:08x}", random_addr);
    loop {
        let val = random::<u32>();
        bridge.poke(random_addr, val)?;
        let cmp = bridge.peek(random_addr)?;
        if cmp != val {
            error!(
                "loop {}: expected {:08x}, got {:08x}",
                loop_counter, val, cmp
            );
            return Err(ServerError::RandomValueError(loop_counter, val, cmp));
        }
        if (loop_counter % 1000) == 0 {
            info!("loop: {} ({:08x})", loop_counter, val);
        }
        loop_counter = loop_counter.wrapping_add(1);
        if let Some(max_loops) = cfg.random_loops {
            if loop_counter > max_loops {
                info!("no errors encountered");
                return Ok(());
            }
        }
    }
}

pub fn memory_access(cfg: Config, bridge: bridge::Bridge) -> Result<(), ServerError> {
    if let Some(addr) = cfg.memory_address {
        if let Some(value) = cfg.memory_value {
            bridge.poke(addr, value)?;
        } else {
            let val = bridge.peek(addr)?;
            println!("Value at {:08x}: {:08x}", addr, val);
        }
    } else {
        println!("No operation and no address specified!");
        println!(
            "Try specifying an address such as \"0x10000000\".  See --help for more information"
        );
    }
    Ok(())
}

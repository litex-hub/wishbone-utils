use crate::config::{Config, ConfigError};
use crate::gdb;
use crate::riscv;
use crate::wishbone;

use byteorder::{LittleEndian, ReadBytesExt};
use log::{error, info};
use rand::prelude::*;
use wishbone_bridge::{Bridge, BridgeError};

use std::fs::File;
use std::io;
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

mod utra;
use utra::*;
use indicatif::{ProgressBar, ProgressStyle};

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ServerKind {
    /// DevMem2 equivalent
    MemoryAccess,

    /// Wishbone bridge
    Wishbone,

    /// GDB server
    GDB,

    /// Send random data back and forth
    RandomTest,

    /// Load a file into memory
    LoadFile,

    /// Run a terminal
    Terminal,

    /// View the messible
    Messible,

    /// Flash programming
    FlashProgram,
}

#[derive(Debug)]
pub enum ServerError {
    IoError(io::Error),
    WishboneError(wishbone::WishboneServerError),
    GdbError(gdb::GdbServerError),
    BridgeError(BridgeError),
    RiscvCpuError(riscv::RiscvCpuError),
    RandomValueError(
        u32, /* counter */
        u32, /* expected */
        u32, /* observed */
    ),
    TerminalError(terminal::error::ErrorKind),

    /// The specified address was not in mappable range
    UnmappableAddress(String),
    FlashError(
        u32,  // expected
        u32,  // observed
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
impl std::convert::From<BridgeError> for ServerError {
    fn from(e: BridgeError) -> ServerError {
        ServerError::BridgeError(e)
    }
}
impl std::convert::From<riscv::RiscvCpuError> for ServerError {
    fn from(e: riscv::RiscvCpuError) -> ServerError {
        ServerError::RiscvCpuError(e)
    }
}

impl std::convert::From<terminal::error::ErrorKind> for ServerError {
    fn from(e: terminal::error::ErrorKind) -> ServerError {
        ServerError::TerminalError(e)
    }
}

impl ServerKind {
    pub fn from_string(item: &str) -> Result<ServerKind, ConfigError> {
        match item {
            "gdb" => Ok(ServerKind::GDB),
            "wishbone" => Ok(ServerKind::Wishbone),
            "random-test" => Ok(ServerKind::RandomTest),
            "load-file" => Ok(ServerKind::LoadFile),
            "terminal" => Ok(ServerKind::Terminal),
            "messible" => Ok(ServerKind::Messible),
            "memory-access" => Ok(ServerKind::MemoryAccess),
            "flash-program" => Ok(ServerKind::FlashProgram),
            unknown => Err(ConfigError::UnknownServerKind(unknown.to_owned())),
        }
    }
}

/// Poll the Messible at the address specified.
/// Return `true` if there is still data to be read
/// after returning.
fn poll_messible(
    messible_address: Option<u32>,
    bridge: &Bridge,
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

/// Poll the UART at the address specified.
/// Return `true` if there is still data to be read
/// after returning.
fn poll_uart(uart_address: u32, bridge: &Bridge) -> Result<bool, BridgeError> {
    Ok(bridge.peek(uart_address)? == 0)
}

pub fn gdb_server(cfg: &Config, bridge: Bridge) -> Result<(), ServerError> {
    let cpu = riscv::RiscvCpu::new(&bridge, cfg.debug_offset)?;
    // Enable messible support, but only if we're not also running a messible or wishbone server.
    let messible_address = if cfg.server_kind.contains(&ServerKind::Messible)
        || cfg.server_kind.contains(&ServerKind::Wishbone)
    {
        None
    } else {
        cfg.messible_address
    };
    loop {
        let connection = {
            let listener = match TcpListener::bind(format!("{}:{}", cfg.bind_addr, cfg.gdb_port)) {
                Ok(o) => o,
                Err(e) => {
                    error!("couldn't bind to address: {:?}", e);
                    return Err(ServerError::IoError(e));
                }
            };

            // accept connections and process them serially
            info!(
                "accepting gdb connections on {}:{}",
                cfg.bind_addr, cfg.gdb_port
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
                            do_pause =
                                !poll_messible(messible_address, &poll_bridge, &mut gdb_controller);
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

pub fn wishbone_server(cfg: &Config, bridge: Bridge) -> Result<(), ServerError> {
    let mut wishbone = wishbone::WishboneServer::new(&cfg).unwrap();
    // Enable messible support, but only if we're not also running a messible server.
    let messible_address = if cfg.server_kind.contains(&ServerKind::Messible) {
        None
    } else {
        cfg.messible_address
    };

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

                // If there's no more data, pause for a short time.
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

pub fn random_test(cfg: &Config, bridge: Bridge) -> Result<(), ServerError> {
    let mut loop_counter: u32 = 0;
    let random_addr = match cfg.random_address {
        Some(s) => s,
        None => 0x1000_0000 + 8192,
    };
    let random_range = match cfg.random_range {
        Some(s) => s,
        None => 0,
    };
    info!(
        "writing random values to 0x{:08x} - 0x{:08x}",
        random_addr,
        random_addr + random_range
    );
    loop {
        let val = random::<u32>();
        let extra_addr = match cfg.random_range {
            Some(s) => (random::<u32>() % s) & !3,
            None => 0,
        };
        bridge.poke(random_addr + extra_addr, val)?;
        let cmp = bridge.peek(random_addr + extra_addr)?;
        if cmp != val {
            error!(
                "loop {} @ 0x{:08x}: expected 0x{:08x}, got 0x{:08x}",
                loop_counter,
                random_addr + extra_addr,
                val,
                cmp
            );
            return Err(ServerError::RandomValueError(loop_counter, val, cmp));
        }
        if (loop_counter % 1000) == 0 {
            info!(
                "loop: {} @ 0x{:08x} (0x{:08x})",
                loop_counter,
                extra_addr + random_addr,
                val
            );
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

pub fn memory_access(cfg: &Config, bridge: Bridge) -> Result<(), ServerError> {
    if let Some(addr) = cfg.memory_address {
        if let Some(value) = cfg.memory_value {
            if cfg.burst_length == 4 {
                bridge.poke(addr, value)?;
            }
        } else if let Some(file_name) = &cfg.burst_source {
            use std::io::Read;
            info!("Loading contents of {} to 0x{:08x}", file_name, addr);
            let mut f = File::open(file_name)?;
            let mut data: Vec<u8> = vec![];
            f.read_to_end(&mut data)?;
            info!("Sending {} bytes", data.len());
            bridge.burst_write(addr, &data)?;
        } else {
            if cfg.burst_length == 4 {
                let val = bridge.peek(addr)?;
                println!("Value at {:08x}: {:08x}", addr, val);
            } else {
                let page = bridge.burst_read(addr, cfg.burst_length);
                match page {
                    Ok(array) => {
                        if cfg.hexdump {
                            for i in 0..array.len() {
                                if (i % 16) == 0 {
                                    println!(); // carriage return
                                    print!("{:08x}: ", addr as usize + i);
                                }
                                print!("{:02x} ", array[i]);
                            }
                            println!("");
                        } else {
                            use std::io::Write;
                            io::stdout().write_all(&array)?;
                        }
                    },
                    _ => {
                        error!("Error occured reading page");
                    }
                }
            }
        }
    } else {
        println!("No operation and no address specified!");
        println!(
            "Try specifying an address such as \"0x10000000\".  See --help for more information"
        );
    }
    Ok(())
}

pub fn load_file(cfg: &Config, bridge: Bridge) -> Result<(), ServerError> {
    let mut word_counter: u32 = 0;
    if let Some(file_name) = &cfg.load_name {
        if let Some(addr) = cfg.load_addr {
            let mut f = File::open(file_name)?;
            let f_len = f.metadata().unwrap().len() as u32;
            info!("Loading {} bytes from {} to address 0x{:08x}", f_len, file_name, addr);
            while word_counter < f_len {
                let value = match f.read_u32::<LittleEndian>() {
                    Ok(x) => x,
                    Err(e) => {
                        error!("Error reading: {}", e);
                        return Ok(());
                    }
                };
                if (word_counter % 1024) == 0 {
                    info!(
                        "write to {:08x}: ({:08x}) - {}%",
                        addr + word_counter,
                        value,
                        (word_counter * 100 / f_len)
                    );
                }
                bridge.poke(addr + word_counter, value)?;
                word_counter = word_counter.wrapping_add(4);
            }
            info!("Done. Wrote {} bytes", word_counter);
        } else {
            error!("No load address specified");
        }
    } else {
        println!("No filename specified!");
    }
    Ok(())
}

// demo of burn performance: https://asciinema.org/a/j2HfItVBwRbdimuFMvplRA4DT
pub fn flash_program(cfg: &Config, bridge: Bridge) -> Result<(), ServerError> {
    let spinor_base: u32;
    let flash_region: u32;
    let reset_addr: u32;
    let vexriscv_debug_addr: u32;
    spinor_base = cfg
        .register_mapping
        .get("spinor")
        .ok_or(ServerError::UnmappableAddress("spinor".to_string()))?.unwrap();
    flash_region = cfg
        .register_mapping
        .get("spiflash")
        .ok_or(ServerError::UnmappableAddress("spiflash".to_string()))?.unwrap();
    reset_addr = cfg
        .register_mapping
        .get("reboot_cpu_reset")
        .ok_or(ServerError::UnmappableAddress("reboot_cpu_reset".to_string()))?.unwrap();
    vexriscv_debug_addr = cfg
        .register_mapping
        .get("vexriscv_debug")
        .ok_or(ServerError::UnmappableAddress("vexriscv_debug".to_string()))?.unwrap();

    if let Some(file_name) = &cfg.load_name {
        if let Some(addr) = cfg.load_addr {

            use std::io::Read;
            info!("Burning contents of {} to 0x{:08x}", file_name, addr);
            let mut f = File::open(file_name)?;
            let mut data: Vec<u8> = vec![];
            f.read_to_end(&mut data)?;
            info!("{} total bytes", data.len());
            if data.len() == 0 {
                return Ok(());
            }

            if addr + data.len() as u32 >= 0x0800_0000 {
                error!("Write data out of bounds! Aborting.");
                return Err(ServerError::UnmappableAddress((addr + data.len() as u32).to_string()));
            }

            // note to those referring to this as reference code for local hardware:
            // WIP bit must be consulted when running from the local CPU, as it runs much faster
            // than the command state machines can finish. However, via USB we can safely assume
            // all commands complete issuing before the next USB packet can arrive.

            let flash_rdsr = |lock_reads: u32| {
                let mut spinor_csr = spinor::CSR::new(spinor_base as *mut u32);
                bridge.poke(spinor_base + (spinor::CMD_ARG.offset as u32) * 4, 0)?;
                bridge.poke(spinor_base + (spinor::COMMAND.offset as u32) * 4,
                      spinor_csr.ms(spinor::COMMAND_EXEC_CMD, 1)
                    | spinor_csr.ms(spinor::COMMAND_LOCK_READS, lock_reads)
                    | spinor_csr.ms(spinor::COMMAND_CMD_CODE, 0x05) // RDSR
                    | spinor_csr.ms(spinor::COMMAND_DUMMY_CYCLES, 4)
                    | spinor_csr.ms(spinor::COMMAND_DATA_WORDS, 1)
                    | spinor_csr.ms(spinor::COMMAND_HAS_ARG, 1)
                )?;
                bridge.peek(spinor_base + (spinor::CMD_RBK_DATA.offset as u32) * 4)
            };

            let flash_rdscur = || {
                let mut spinor_csr = spinor::CSR::new(spinor_base as *mut u32);
                bridge.poke(spinor_base + (spinor::CMD_ARG.offset as u32) * 4, 0)?;
                bridge.poke(spinor_base + (spinor::COMMAND.offset as u32) * 4,
                      spinor_csr.ms(spinor::COMMAND_EXEC_CMD, 1)
                    | spinor_csr.ms(spinor::COMMAND_LOCK_READS, 1)
                    | spinor_csr.ms(spinor::COMMAND_CMD_CODE, 0x2B) // RDSCUR
                    | spinor_csr.ms(spinor::COMMAND_DUMMY_CYCLES, 4)
                    | spinor_csr.ms(spinor::COMMAND_DATA_WORDS, 1)
                    | spinor_csr.ms(spinor::COMMAND_HAS_ARG, 1)
                )?;
                bridge.peek(spinor_base + (spinor::CMD_RBK_DATA.offset as u32) * 4)
            };

            let flash_rdid = |offset: u32| {
                let mut spinor_csr = spinor::CSR::new(spinor_base as *mut u32);
                bridge.poke(spinor_base + (spinor::CMD_ARG.offset as u32) * 4, 0)?;
                bridge.poke(spinor_base + (spinor::COMMAND.offset as u32) * 4,
                    spinor_csr.ms(spinor::COMMAND_EXEC_CMD, 1)
                  | spinor_csr.ms(spinor::COMMAND_CMD_CODE, 0x9f)  // RDID
                  | spinor_csr.ms(spinor::COMMAND_DUMMY_CYCLES, 4)
                  | spinor_csr.ms(spinor::COMMAND_DATA_WORDS, offset) // 2 -> 0x3b3b8080, // 1 -> 0x8080c2c2
                  | spinor_csr.ms(spinor::COMMAND_HAS_ARG, 1)
                )?;
                bridge.peek(spinor_base + (spinor::CMD_RBK_DATA.offset as u32) * 4)
            };

            let flash_wren = || {
                let mut spinor_csr = spinor::CSR::new(spinor_base as *mut u32);
                bridge.poke(spinor_base + (spinor::CMD_ARG.offset as u32) * 4, 0)?;
                bridge.poke(spinor_base + (spinor::COMMAND.offset as u32) * 4,
                    spinor_csr.ms(spinor::COMMAND_EXEC_CMD, 1)
                  | spinor_csr.ms(spinor::COMMAND_CMD_CODE, 0x06)  // WREN
                  | spinor_csr.ms(spinor::COMMAND_LOCK_READS, 1)
                )
            };

            let flash_wrdi = || {
                let mut spinor_csr = spinor::CSR::new(spinor_base as *mut u32);
                bridge.poke(spinor_base + (spinor::CMD_ARG.offset as u32) * 4, 0)?;
                bridge.poke(spinor_base + (spinor::COMMAND.offset as u32) * 4,
                    spinor_csr.ms(spinor::COMMAND_EXEC_CMD, 1)
                  | spinor_csr.ms(spinor::COMMAND_CMD_CODE, 0x04)  // WRDI
                  | spinor_csr.ms(spinor::COMMAND_LOCK_READS, 1)
                )
            };

            let flash_se4b = |sector_address: u32| {
                let mut spinor_csr = spinor::CSR::new(spinor_base as *mut u32);
                bridge.poke(spinor_base + (spinor::CMD_ARG.offset as u32) * 4, sector_address)?;
                bridge.poke(spinor_base + (spinor::COMMAND.offset as u32) * 4,
                    spinor_csr.ms(spinor::COMMAND_EXEC_CMD, 1)
                  | spinor_csr.ms(spinor::COMMAND_CMD_CODE, 0x21)  // SE4B
                  | spinor_csr.ms(spinor::COMMAND_HAS_ARG, 1)
                  | spinor_csr.ms(spinor::COMMAND_LOCK_READS, 1)
                )
            };

            let flash_be4b = |block_address: u32| {
                let mut spinor_csr = spinor::CSR::new(spinor_base as *mut u32);
                bridge.poke(spinor_base + (spinor::CMD_ARG.offset as u32) * 4, block_address)?;
                bridge.poke(spinor_base + (spinor::COMMAND.offset as u32) * 4,
                    spinor_csr.ms(spinor::COMMAND_EXEC_CMD, 1)
                  | spinor_csr.ms(spinor::COMMAND_CMD_CODE, 0xdc)  // BE4B
                  | spinor_csr.ms(spinor::COMMAND_HAS_ARG, 1)
                  | spinor_csr.ms(spinor::COMMAND_LOCK_READS, 1)
                )
            };

            let flash_pp4b = |address: u32, data_bytes: u32| {
                let mut spinor_csr = spinor::CSR::new(spinor_base as *mut u32);
                bridge.poke(spinor_base + (spinor::CMD_ARG.offset as u32) * 4, address)?;
                bridge.poke(spinor_base + (spinor::COMMAND.offset as u32) * 4,
                    spinor_csr.ms(spinor::COMMAND_EXEC_CMD, 1)
                  | spinor_csr.ms(spinor::COMMAND_CMD_CODE, 0x12)  // PP4B
                  | spinor_csr.ms(spinor::COMMAND_HAS_ARG, 1)
                  | spinor_csr.ms(spinor::COMMAND_DATA_WORDS, data_bytes / 2)
                  | spinor_csr.ms(spinor::COMMAND_LOCK_READS, 1)
                )
            };

            info!("Halting CPU.");
            bridge.poke(vexriscv_debug_addr, 0x00020000)?; // halt the CPU

            ///////// ID code check
            let code = flash_rdid(1)?;
            info!("ID code bytes 1-2: 0x{:08x}", code);
            if code != 0x8080c2c2 {
                error!("ID code mismatch");
                return Err(ServerError::FlashError(0x8080c2c2, code));
            }
            let code = flash_rdid(2)?;
            info!("ID code bytes 2-3: 0x{:08x}", code);
            if code != 0x3b3b8080 {
                error!("ID code mismatch");
                return Err(ServerError::FlashError(0x3b3b8080, code));
            }

            //////// block erase
            let mut erased = 0;
            let pb = ProgressBar::new(data.len() as u64);
            pb.set_style(ProgressStyle::default_bar()
            .template("{spinner:.yellow} [{elapsed_precise}] [{bar:40.red/magenta}] {bytes}/{total_bytes} ({eta})")
            .progress_chars("#>-"));
            while erased < data.len() {
                let blocksize;
                if data.len() - erased > 4096 {
                    blocksize = 4096;
                } else {
                    blocksize = 65536;
                }

                loop {
                    flash_wren()?;
                    let status = flash_rdsr(1)?;
                    // println!("WREN: FLASH status register: 0x{:08x}", status);
                    if status & 0x02 != 0 {
                        break;
                    }
                }

                if blocksize <= 4096 {
                    flash_se4b(addr + erased as u32)?;
                } else {
                    flash_be4b(addr + erased as u32)?;
                }
                erased += blocksize;

                loop {
                    let status = flash_rdsr(1)?;
                    // println!("BE4B: FLASH status register: 0x{:08x}", status);
                    if status & 0x01 == 0 {
                        break;
                    }
                }

                let result = flash_rdscur()?;
                // println!("erase result: 0x{:08x}", result);
                if result & 0x60 != 0 {
                    error!("E_FAIL/P_FAIL set, programming may have failed.")
                }

                if flash_rdsr(1)? & 0x02 != 0 {
                    flash_wrdi()?;
                    loop {
                        let status = flash_rdsr(1)?;
                        // println!("WRDI: FLASH status register: 0x{:08x}", status);
                        if status & 0x02 == 0 {
                            break;
                        }
                    }
                }
                // use "min" because we erase block size is typically not evenly divided with program size
                pb.set_position(std::cmp::min(erased, data.len()) as u64);
            }
            pb.finish_with_message("Erase finished");

            ////////// program
            // pre-load the page program buffer. note that data.len() must be even
            if data.len() % 4 != 0 { // add "blank" bytes to the end to get us to a 32-bit aligned number of bytes
                if data.len() % 4 == 1 {
                    data.push(0xff);
                    data.push(0xff);
                    data.push(0xff);
                }
                if data.len() % 4 == 2 {
                    data.push(0xff);
                    data.push(0xff);
                }
                if data.len() % 4 == 3 {
                    data.push(0xff);
                }
            }

            let mut written = 0;

            let pb = ProgressBar::new(data.len() as u64);
            pb.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .progress_chars("#>-"));
            while written < data.len() {
                let chunklen: usize;
                if data.len() - written > 256 {
                    chunklen = 256;
                } else {
                    chunklen = data.len() - written;
                }

                loop {
                    flash_wren()?;
                    let status = flash_rdsr(1)?;
                    // println!("WREN: FLASH status register: 0x{:08x}", status);
                    if status & 0x02 != 0 {
                        break;
                    }
                }

                let mut page: Vec<u8> = vec![];
                for i in 0..chunklen {
                    page.push(data[written + i]);
                    // println!("program: index {}, 0x{:02x}", i, data[written + i]);
                }
                bridge.burst_write(flash_region, &page)?;

                // info!("PP4B: processing chunk of length {} bytes from offset 0x{:08x}", chunklen, 0x80_0000 + written);
                flash_pp4b(addr + written as u32, chunklen as u32)?;

                if cfg.careful_flashing {
                    loop {
                        let status = flash_rdsr(1)?;
                        // println!("PP4B: FLASH status register: 0x{:08x}", status);
                        if status & 0x01 == 0 {
                            break;
                        }
                    }
                    let result = flash_rdscur()?;
                    // println!("program result: 0x{:08x}", result);
                    if result & 0x60 != 0 {
                        error!("E_FAIL/P_FAIL set, programming may have failed.")
                    }
                }
                written += chunklen;
                pb.set_position(written as u64);
            }
            pb.finish_with_message("Write finished");


            if flash_rdsr(1)? & 0x02 != 0 {
                flash_wrdi()?;
                loop {
                    let status = flash_rdsr(1)?;
                    // println!("WRDI: FLASH status register: 0x{:08x}", status);
                    if status & 0x02 == 0 {
                        break;
                    }
                }
            }

            // dummy reads to clear the "read lock" bit
            flash_rdsr(0)?;

            /////////// verify
            info!("Performing readback for verification...");
            let page = bridge.burst_read(addr + flash_region, data.len() as u32);
            info!("Comparing results...");
            match page {
                Ok(array) => {
                    let mut error_count = 0;
                    for i in 0..array.len() {
                        if data[i] != array[i] {
                            error_count += 1;
                        }
                    }
                    if error_count != 0 {
                        info!("{} errors found in verification, programming failed", error_count);
                    } else {
                        info!("No errors found, programming passed");
                    }
                },
                _ => {
                    error!("Low-level error occured during verification readback.");
                }
            }
            bridge.poke(vexriscv_debug_addr, 0x02000000)?; // resume the CPU
            info!("Resuming CPU.");

            ////////// reset the CPU, under the presumption that code has changed and we should restart the CPU
            if !cfg.flash_no_reset {
                info!("Resetting CPU.");
                bridge.poke(reset_addr, 1)?;
            }
        } else {
            error!("No target address specified");
        }
    } else {
        println!("No filename specified!");
    }
    Ok(())
}

use terminal::{Action, Event, KeyCode, KeyEvent, KeyModifiers, Retrieved, Terminal, Value};
struct IOInterface {
    term: Terminal<std::io::Stdout>,
    capture_mouse: bool,
}

pub fn terminal_client(cfg: &Config, bridge: Bridge) -> Result<(), ServerError> {
    let poll_time = 10;
    let my_terminal = IOInterface::new(cfg.terminal_mouse);
    use std::io::stdout;
    use std::io::Write;

    let xover_rxtx = cfg
        .register_mapping
        .get("uart_xover_rxtx")
        .map_or(Ok(0xe000_1818), |e| {
            e.ok_or(ServerError::UnmappableAddress("uart_xover_rxtx".to_owned()))
        })?;
    let xover_rxempty =
        cfg.register_mapping
            .get("uart_xover_rxempty")
            .map_or(Ok(0xe000_1820), |e| {
                e.ok_or(ServerError::UnmappableAddress(
                    "uart_xover_rxempty".to_owned(),
                ))
            })?;

    loop {
        if poll_uart(xover_rxempty, &bridge)? {
            let mut char_buffer = vec![];
            let mut read_count = 0;
            while bridge.peek(xover_rxempty)? == 0 && read_count < 100 {
                read_count += 1;
                char_buffer.push(bridge.peek(xover_rxtx)? as u8);
            }
            print!("{}", String::from_utf8_lossy(&char_buffer));
            stdout().flush().ok();
        }

        if let Retrieved::Event(event) = my_terminal
            .term
            .get(Value::Event(Some(Duration::from_millis(poll_time))))?
        {
            match event {
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                })) => return Ok(()),
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                })) => {
                    bridge.poke(xover_rxtx, '\r' as u32)?;
                    bridge.poke(xover_rxtx, '\n' as u32)?;
                }
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                })) => return Ok(()),
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char(e),
                    ..
                })) => bridge.poke(xover_rxtx, e as u32)?,
                Some(_event) => {
                    // println!("{:?}\r", event);
                }
                None => (),
            }
        }
    }
}

impl IOInterface {
    pub fn new(capture_mouse: bool) -> IOInterface {
        let term = terminal::stdout();
        term.act(Action::EnableRawMode)
            .expect("can't enable raw mode");
        if capture_mouse {
            term.act(Action::EnableMouseCapture)
                .expect("can't capture mouse");
        }
        IOInterface {
            term,
            capture_mouse,
        }
    }
}
impl Drop for IOInterface {
    fn drop(&mut self) {
        if self.capture_mouse {
            self.term.act(Action::DisableMouseCapture).ok();
        }
        self.term.act(Action::DisableRawMode).ok();
    }
}

pub fn messible_client(cfg: &Config, bridge: Bridge) -> Result<(), ServerError> {
    let poll_time = 10;
    let my_terminal = IOInterface::new(cfg.terminal_mouse);
    use std::io::stdout;
    use std::io::Write;

    let messible_base = cfg.messible_address.unwrap_or(0xe000_8000);

    loop {
        let mut char_buffer = vec![];
        let mut read_count = 0;
        while bridge.peek(messible_base + 8)? & 0x2 == 2 && read_count < 100 {
            read_count += 1;
            char_buffer.push(bridge.peek(messible_base + 4)? as u8);
        }
        if !char_buffer.is_empty() {
            print!("{}", String::from_utf8_lossy(&char_buffer));
            stdout().flush().ok();
        }

        if let Retrieved::Event(event) = my_terminal
            .term
            .get(Value::Event(Some(Duration::from_millis(poll_time))))?
        {
            match event {
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                })) => return Ok(()),
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                })) => return Ok(()),
                Some(_event) => (),
                None => (),
            }
        }
    }
}

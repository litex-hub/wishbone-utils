extern crate byteorder;
use std::io;
use std::io::{Read, Write};
use std::net::TcpStream;

use super::riscv::{RiscvCpu, RiscvCpuError};
use wishbone_bridge::{Bridge, BridgeError};

use log::{debug, error, info};

use crate::gdb::byteorder::ByteOrder;
use byteorder::{BigEndian, NativeEndian};

const SUPPORTED_QUERIES: &[u8] = b"PacketSize=3fff;qXfer:features:read+;qXfer:threads:read+;qXfer:memory-map:read-;QStartNoAckMode+;vContSupported+";

pub struct GdbController {
    connection: TcpStream,
}

impl Write for GdbController {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.connection.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.connection.flush()
    }
}

impl GdbController {
    pub fn gdb_send(&mut self, inp: &[u8]) -> io::Result<()> {
        let mut buffer = [0; 16388];
        let mut checksum: u8 = 0;
        buffer[0] = b'$';
        for i in 0..inp.len() {
            buffer[i + 1] = inp[i];
            checksum = checksum.wrapping_add(inp[i]);
        }
        let checksum_str = &format!("{:02x}", checksum);
        let checksum_bytes = checksum_str.as_bytes();
        buffer[inp.len() + 1] = b'#';
        buffer[inp.len() + 2] = checksum_bytes[0];
        buffer[inp.len() + 3] = checksum_bytes[1];
        let (to_write, _rest) = buffer.split_at(inp.len() + 4);
        debug!(
            " > Writing {} bytes: {}",
            to_write.len(),
            String::from_utf8_lossy(&to_write)
        );
        self.connection.write_all(&to_write)?;
        Ok(())
    }

    pub fn print_string(&mut self, msg: &str) -> io::Result<()> {
        debug!("Printing string {} to GDB", msg);
        let mut strs: Vec<String> = msg
            .as_bytes()
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect();
        strs.insert(0, "O".to_string());
        let joined = strs.join("");
        self.gdb_send(joined.as_bytes())
    }
}

pub struct GdbServer {
    connection: TcpStream,
    no_ack_mode: bool,
    is_alive: bool,
    last_signal: u8,
}

fn swab(src: u32) -> u32 {
    (src << 24) & 0xff00_0000
        | (src << 8) & 0x00ff_0000
        | (src >> 8) & 0x0000_ff00
        | (src >> 24) & 0x0000_00ff
}

pub fn parse_u32(value: &str) -> Result<u32, GdbServerError> {
    match u32::from_str_radix(value, 16) {
        Ok(o) => Ok(o),
        Err(e) => Err(GdbServerError::NumberParseError(value.to_owned(), e)),
    }
}

pub fn parse_i32(value: &str) -> Result<i32, GdbServerError> {
    match i32::from_str_radix(value, 16) {
        Ok(o) => Ok(o),
        Err(e) => Err(GdbServerError::NumberParseError(value.to_owned(), e)),
    }
}

fn gdb_unescape(input: &[u8]) -> Vec<u8> {
    let mut out = input.to_vec();
    out.iter_mut().fold(&mut Vec::new(), |vec_acc, this_u8| {
        if vec_acc.last() == Some(&(b'}')) {
            let len = vec_acc.len();
            vec_acc[len - 1] = *this_u8 ^ 0x20;
        } else {
            vec_acc.push(*this_u8);
        }
        vec_acc
    });
    out
}

pub fn parse_u64(value: &str) -> Result<u64, GdbServerError> {
    match u64::from_str_radix(value, 16) {
        Ok(o) => Ok(o),
        Err(e) => Err(GdbServerError::NumberParseError(value.to_owned(), e)),
    }
}

#[derive(Debug)]
pub enum GdbServerError {
    /// Rust standard IO error
    IoError(io::Error),

    /// The network connection has closed
    ConnectionClosed,

    /// We were unable to parse an integer
    NumberParseError(String, std::num::ParseIntError),

    /// Something happened with the CPU
    CpuError(RiscvCpuError),

    /// The bridge failed somehow
    BridgeError(BridgeError),

    /// Something strange was received
    ProtocolError,

    /// Client tried to give us a breakpoint we didn't recognize
    UnknownBreakpointType(String),
}

impl std::convert::From<BridgeError> for GdbServerError {
    fn from(e: BridgeError) -> Self {
        GdbServerError::BridgeError(e)
    }
}

impl std::convert::From<RiscvCpuError> for GdbServerError {
    fn from(e: RiscvCpuError) -> Self {
        GdbServerError::CpuError(e)
    }
}

impl std::convert::From<io::Error> for GdbServerError {
    fn from(e: io::Error) -> Self {
        GdbServerError::IoError(e)
    }
}

#[derive(Debug, PartialEq)]
pub enum BreakPointType {
    BreakSoft,
    BreakHard,
    WatchWrite,
    WatchRead,
    WatchAccess,
}

impl BreakPointType {
    fn from_str(r: &str) -> Result<BreakPointType, GdbServerError> {
        match r {
            "0" => Ok(BreakPointType::BreakSoft),
            "1" => Ok(BreakPointType::BreakHard),
            "2" => Ok(BreakPointType::WatchWrite),
            "3" => Ok(BreakPointType::WatchRead),
            "4" => Ok(BreakPointType::WatchAccess),
            c => Err(GdbServerError::UnknownBreakpointType(c.to_string())),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum GdbCommand {
    /// Server gave an unrecognized command
    Unknown(String),

    /// This should be responded to in the same way as Unknown(String),
    /// sent by the server to test how it responds to unknown packets.
    MustReplyEmpty,

    /// qSupported
    SupportedQueries(String),

    /// QStartNoAckMode
    StartNoAckMode,

    /// D
    Disconnect,

    /// Hg#
    SetCurrentThread(u64),

    /// Hc# (# may be -1)
    ContinueThread(i32),

    /// ?
    LastSignalPacket,

    /// qfThreadInfo
    GetThreadInfo,

    /// qC
    GetCurrentThreadId,

    /// qAttached
    CheckIsAttached,

    /// g
    GetRegisters,

    /// p#
    GetRegister(u32),

    /// P#=#
    SetRegister(u32, u32),

    /// qSymbol::
    SymbolsReady,

    /// m#,#
    ReadMemory(u32 /* addr */, u32 /* length */),

    /// M#,#:#
    WriteMemory(
        u32,      /* addr */
        u32,      /* length */
        Vec<u32>, /* value */
    ),

    /// vCont?
    VContQuery,

    /// vCont;c
    VContContinue,

    /// vCont;C04:0;c
    VContContinueFromSignal(String),

    /// vCont;s:0;c
    VContStepFromSignal(String),

    /// c
    Continue,

    /// s
    Step,

    /// Ctrl-C
    Interrupt,

    /// qRcmd,
    MonitorCommand(String),

    /// Z0,###,2
    AddBreakpoint(
        BreakPointType,
        u32, /* address */
        u32, /* length */
    ),

    /// z0,###,2
    RemoveBreakpoint(
        BreakPointType,
        u32, /* address */
        u32, /* length */
    ),

    /// qOffsets
    GetOffsets,

    /// qXfer:memory-map:read::
    ReadMemoryMap(u32 /* offset */, u32 /* len */),

    /// qXfer:features:read:target.xml:0,1000
    ReadFeature(
        String, /* filename */
        u32,    /* offset */
        u32,    /* len */
    ),

    /// qTStatus
    TraceStatusQuery,

    /// qXfer:threads:read::0,1000
    ReadThreads(u32 /* offset */, u32 /* len */),
}

impl GdbServer {
    pub fn new(connection: TcpStream) -> Result<GdbServer, GdbServerError> {
        Ok(GdbServer {
            connection,
            no_ack_mode: false,
            is_alive: true,
            last_signal: 0,
        })
    }

    #[allow(clippy::cognitive_complexity)]
    fn packet_to_command(&self, raw_pkt: &[u8]) -> Result<GdbCommand, GdbServerError> {
        let pkt = String::from_utf8_lossy(raw_pkt).to_string();
        debug!("Raw GDB packet: {}", pkt);

        if pkt == "qSupported" || pkt.starts_with("qSupported:") {
            Ok(GdbCommand::SupportedQueries(pkt))
        } else if pkt == "D" {
            Ok(GdbCommand::Disconnect)
        } else if pkt == "QStartNoAckMode" {
            Ok(GdbCommand::StartNoAckMode)
        } else if pkt == "qAttached" {
            Ok(GdbCommand::CheckIsAttached)
        } else if pkt == "qOffsets" {
            Ok(GdbCommand::GetOffsets)
        } else if pkt == "qTStatus" {
            Ok(GdbCommand::TraceStatusQuery)
        } else if pkt.starts_with("qXfer:memory-map:read::") {
            let pkt = pkt.trim_start_matches("qXfer:memory-map:read::");
            let offsets: Vec<&str> = pkt.split(',').collect();
            let offset = parse_u32(offsets[0])?;
            let len = parse_u32(offsets[1])?;
            Ok(GdbCommand::ReadMemoryMap(offset, len))
        } else if pkt.starts_with("qXfer:features:read:") {
            let pkt = pkt.trim_start_matches("qXfer:features:read:");
            let fields: Vec<&str> = pkt.split(':').collect();
            let offsets: Vec<&str> = fields[1].split(',').collect();
            let offset = parse_u32(offsets[0])?;
            let len = parse_u32(offsets[1])?;
            Ok(GdbCommand::ReadFeature(fields[0].to_string(), offset, len))
        } else if pkt.starts_with("qXfer:threads:read::") {
            let pkt = pkt.trim_start_matches("qXfer:threads:read::");
            let offsets: Vec<&str> = pkt.split(',').collect();
            let offset = parse_u32(offsets[0])?;
            let len = parse_u32(offsets[1])?;
            Ok(GdbCommand::ReadThreads(offset, len))
        } else if pkt.starts_with('Z') {
            let pkt = pkt.trim_start_matches('Z');
            let fields: Vec<&str> = pkt.split(',').collect();
            let bptype = BreakPointType::from_str(fields[0])?;
            let address = parse_u32(fields[1])?;
            let size = parse_u32(fields[2])?;
            Ok(GdbCommand::AddBreakpoint(bptype, address, size))
        } else if pkt.starts_with('z') {
            let pkt = pkt.trim_start_matches('z');
            let fields: Vec<&str> = pkt.split(',').collect();
            let bptype = BreakPointType::from_str(fields[0])?;
            let address = parse_u32(fields[1])?;
            let size = parse_u32(fields[2])?;
            Ok(GdbCommand::RemoveBreakpoint(bptype, address, size))
        } else if pkt.starts_with("qRcmd,") {
            let pkt = pkt.trim_start_matches("qRcmd,");
            let pkt_bytes = pkt.as_bytes();
            let mut tmp1 = Vec::new();
            let mut acc = 0;
            for (i, pkt_byte) in pkt_bytes.iter().enumerate() {
                let nybble = if *pkt_byte >= 0x30 && *pkt_byte <= 0x39 {
                    *pkt_byte - 0x30
                } else if *pkt_byte >= 0x61 && *pkt_byte <= 0x66 {
                    *pkt_byte + 10 - 0x61
                } else if *pkt_byte >= 0x41 && *pkt_byte <= 0x46 {
                    *pkt_byte + 10 - 0x41
                } else {
                    0
                };
                if i & 1 == 1 {
                    tmp1.push((acc << 4) | nybble);
                    acc = 0;
                } else {
                    acc = nybble;
                }
            }
            Ok(GdbCommand::MonitorCommand(
                String::from_utf8_lossy(&tmp1).to_string(),
            ))
        } else if pkt == "g" {
            Ok(GdbCommand::GetRegisters)
        } else if pkt.starts_with('P') {
            let pkt = pkt.trim_start_matches('P').to_string();
            let v: Vec<&str> = pkt.split('=').collect();
            let addr = parse_u32(v[0])?;
            let value = swab(parse_u32(v[1])?);
            Ok(GdbCommand::SetRegister(addr, value))
        } else if pkt == "c" {
            Ok(GdbCommand::Continue)
        } else if pkt == "s" {
            Ok(GdbCommand::Step)
        } else if pkt.starts_with('m') {
            let pkt = pkt.trim_start_matches('m').to_string();
            let v: Vec<&str> = pkt.split(',').collect();
            let addr = parse_u32(v[0])?;
            let length = parse_u32(v[1])?;
            Ok(GdbCommand::ReadMemory(addr, length))
        } else if pkt.starts_with('M') {
            let pkt = pkt.trim_start_matches('M').to_string();
            let d: Vec<&str> = pkt.split(':').collect();
            let v: Vec<&str> = d[0].split(',').collect();
            let addr = parse_u32(v[0])?;
            let length = parse_u32(v[1])?;
            let value = swab(parse_u32(d[1])?);
            Ok(GdbCommand::WriteMemory(addr, length, vec![value]))
        } else if pkt.starts_with('X') {
            let (_opcode, data) = match raw_pkt.split_first() {
                None => return Err(GdbServerError::ProtocolError),
                Some(s) => s,
            };
            // Packet format: Xaddr,count:data
            // Look for ":"
            let mut delimiter_offset = None;
            for (idx, c) in data.iter().enumerate() {
                if *c == b':' {
                    delimiter_offset = Some(idx);
                    break;
                }
            }
            let delimiter_offset = match delimiter_offset {
                Some(s) => s,
                None => return Err(GdbServerError::ProtocolError),
            };
            // warn!("X command: Not doing GDB unescaping");
            let (description, bin_data_plus) = data.split_at(delimiter_offset);
            let bin_data_plus = bin_data_plus.split_first();
            let description = String::from_utf8_lossy(&description).to_string();
            let v: Vec<&str> = description.split(',').collect();
            let addr = parse_u32(v[0])?;
            let length = parse_u32(v[1])?;

            let mut values = vec![];
            if let Some((_delimiter, bin_data)) = bin_data_plus {
                let bin_data = gdb_unescape(bin_data);
                for value in bin_data.chunks_exact(4) {
                    values.push(swab(BigEndian::read_u32(&value)));
                }
                let remainder = bin_data.chunks_exact(4).remainder();
                if !remainder.is_empty() {
                    let mut remainder = remainder.to_vec();
                    while remainder.len() < 4 {
                        remainder.insert(0, 0);
                    }
                    // remainder.resize(4, 0);
                    values.push(swab(BigEndian::read_u32(&remainder)));
                }
            }
            Ok(GdbCommand::WriteMemory(addr, length, values))
        } else if pkt.starts_with('p') {
            Ok(GdbCommand::GetRegister(parse_u32(
                pkt.trim_start_matches('p'),
            )?))
        } else if pkt.starts_with("Hg") {
            Ok(GdbCommand::SetCurrentThread(parse_u64(
                pkt.trim_start_matches("Hg"),
            )?))
        } else if pkt.starts_with("Hc") {
            Ok(GdbCommand::ContinueThread(parse_i32(
                pkt.trim_start_matches("Hc"),
            )?))
        } else if pkt == "qC" {
            Ok(GdbCommand::GetCurrentThreadId)
        } else if pkt == "?" {
            Ok(GdbCommand::LastSignalPacket)
        } else if pkt == "qfThreadInfo" {
            Ok(GdbCommand::GetThreadInfo)
        } else if pkt == "vCont?" {
            Ok(GdbCommand::VContQuery)
        } else if pkt == "vCont;c" || pkt == "vCont;c:0" {
            Ok(GdbCommand::VContContinue)
        } else if pkt.starts_with("vCont;C") {
            //vCont;C04:0;c
            let pkt = pkt.trim_start_matches("vCont;C").to_string();
            // let v: Vec<&str> = pkt.split(',').collect();
            Ok(GdbCommand::VContContinueFromSignal(pkt))
        } else if pkt.starts_with("vCont;s") {
            let pkt = pkt.trim_start_matches("vCont;s").to_string();
            Ok(GdbCommand::VContStepFromSignal(pkt))
        } else if pkt == "qSymbol::" {
            Ok(GdbCommand::SymbolsReady)
        } else if pkt == "vMustReplyEmpty" {
            Ok(GdbCommand::MustReplyEmpty)
        } else {
            info!("unrecognized GDB command: {}", pkt);
            Ok(GdbCommand::Unknown(pkt))
        }
    }

    pub fn get_controller(&self) -> GdbController {
        GdbController {
            connection: self.connection.try_clone().unwrap(),
        }
    }

    pub fn get_command(&mut self) -> Result<GdbCommand, GdbServerError> {
        let cmd = self.do_get_command()?;
        debug!("<  GDB packet: {:?}", cmd);
        Ok(cmd)
    }

    fn do_get_command(&mut self) -> Result<GdbCommand, GdbServerError> {
        let mut buffer = [0; 16384];
        let mut byte = [0; 1];
        let mut remote_checksum = [0; 2];
        let mut buffer_offset = 0;

        // XXX Replace this with a BufReader for performance
        loop {
            let len = self.connection.read(&mut byte)?;
            if len == 0 {
                return Err(GdbServerError::ConnectionClosed);
            }

            match byte[0] {
                0x24 /*'$'*/ => {
                    let mut checksum: u8 = 0;
                    loop {
                        let len = self.connection.read(&mut byte)?;
                        if len == 0 {
                            return Err(GdbServerError::ConnectionClosed);
                        }
                        match byte[0] as char {
                            '#' => {
                                // There's got to be a better way to compare the checksum
                                self.connection.read_exact(&mut remote_checksum)?;
                                let checksum_str = format!("{:02x}", checksum);
                                if checksum_str != String::from_utf8_lossy(&remote_checksum) {
                                    info!(
                                        "Checksum mismatch: Calculated {:?} vs {}",
                                        checksum_str,
                                        String::from_utf8_lossy(&remote_checksum)
                                    );
                                    self.gdb_send_nak()?;
                                } else if !self.no_ack_mode {
                                    self.gdb_send_ack()?;
                                }
                                let (buffer, _remainder) = buffer.split_at(buffer_offset);
                                // debug!("<  Read packet ${:?}#{:#?}", String::from_utf8_lossy(buffer), String::from_utf8_lossy(&remote_checksum));
                                return self.packet_to_command(&buffer);
                            }
                            other => {
                                buffer[buffer_offset] = other as u8;
                                buffer_offset += 1;
                                checksum = checksum.wrapping_add(other as u8);
                            }
                        }
                    }
                }
                0x2b /*'+'*/ => {}
                0x2d /*'-'*/ => {}
                0x3 => return Ok(GdbCommand::Interrupt),
                other => error!("Warning: unrecognied byte received: {}", other),
            }
        }
    }

    #[allow(clippy::cognitive_complexity)]
    pub fn process(
        &mut self,
        cmd: GdbCommand,
        cpu: &RiscvCpu,
        bridge: &Bridge,
    ) -> Result<(), GdbServerError> {
        match cmd {
            GdbCommand::SupportedQueries(_) => self.gdb_send(SUPPORTED_QUERIES)?,
            GdbCommand::StartNoAckMode => {
                self.no_ack_mode = true;
                self.gdb_send(b"OK")?
            }
            GdbCommand::SetCurrentThread(_) => self.gdb_send(b"OK")?,
            GdbCommand::ContinueThread(_) => self.gdb_send(b"OK")?,
            GdbCommand::AddBreakpoint(_bptype, address, _size) => {
                let response = match cpu.add_breakpoint(bridge, address) {
                    Ok(_) => "OK",
                    Err(RiscvCpuError::BreakpointExhausted) => {
                        error!("No available breakpoint found");
                        "E0E"
                    }
                    Err(e) => {
                        error!(
                            "An error occurred while trying to add the breakpoint: {:?}",
                            e
                        );
                        "E0E"
                    }
                };
                self.gdb_send(response.as_bytes())?;
            }
            GdbCommand::TraceStatusQuery => self.gdb_send(b"")?,
            GdbCommand::RemoveBreakpoint(_bptype, address, _size) => {
                cpu.remove_breakpoint(bridge, address)?;
                self.gdb_send(b"OK")?
            }
            GdbCommand::LastSignalPacket => {
                let sig_str = format!("S{:02x}", self.last_signal);
                self.gdb_send(if self.is_alive {
                    sig_str.as_bytes()
                } else {
                    b"W00"
                })?
            }
            GdbCommand::GetThreadInfo => self.gdb_send(b"l")?,
            GdbCommand::GetCurrentThreadId => self.gdb_send(b"QC0")?,
            GdbCommand::CheckIsAttached => self.gdb_send(b"1")?,
            GdbCommand::Disconnect => {
                cpu.resume(bridge)?;
                self.gdb_send(b"OK")?
            }
            GdbCommand::GetRegisters => {
                let mut register_list = String::new();
                for i in cpu.all_cpu_registers() {
                    register_list
                        .push_str(format!("{:08x}", swab(cpu.read_register(bridge, i)?)).as_str());
                }
                self.gdb_send(register_list.as_bytes())?
            }
            GdbCommand::GetRegister(reg) => {
                let response = match cpu.read_register(bridge, reg) {
                    Ok(val) => format!("{:08x}", swab(val)),
                    Err(e) => {
                        error!("Error reading register: {}", e);
                        "E01".to_string()
                    }
                };
                self.gdb_send(response.as_bytes())?
            }
            GdbCommand::SetRegister(reg, val) => {
                let response = match cpu.write_register(bridge, reg, val) {
                    Ok(()) => "OK",
                    Err(_) => "E01",
                };
                self.gdb_send(response.as_bytes())?
            }
            GdbCommand::SymbolsReady => self.gdb_send(b"OK")?,
            GdbCommand::ReadMemory(addr, len) => {
                debug!("Reading memory {:08x}", addr);
                let mut values = vec![];

                let mut out_str = String::new();

                if len == 1 {
                    let val = cpu.read_memory(bridge, addr, 1)? as u8;
                    out_str.push_str(&format!("{:02x}", val));
                    self.gdb_send(out_str.as_bytes())?
                } else if len == 2 {
                    let val = cpu.read_memory(bridge, addr, 2)? as u16;
                    let mut buf = [0; 2];
                    BigEndian::write_u16(&mut buf, val);
                    out_str.push_str(&format!("{:04x}", NativeEndian::read_u16(&buf)));
                    self.gdb_send(out_str.as_bytes())?
                } else if len == 4 {
                    values.push(cpu.read_memory(bridge, addr, 4)?);
                    self.gdb_send_u32(values)?
                } else {
                    for offset in (0..len).step_by(4) {
                        values.push(cpu.read_memory(bridge, addr + offset, 4)?);
                        if addr + offset >= 0xffff_fffc {
                            break;
                        }
                    }
                    self.gdb_send_u32(values)?
                }
            }
            GdbCommand::WriteMemory(addr, len, values) => {
                if len == 1 {
                    debug!("Writing memory {:08x} -> {:08x}", addr, values[0] >> 24);
                    cpu.write_memory(bridge, addr, 1, values[0] >> 24)?;
                } else if len == 2 {
                    debug!("Writing memory {:08x} -> {:08x}", addr, values[0] >> 16);
                    cpu.write_memory(bridge, addr, 2, values[0] >> 16)?;
                } else if len == 4 {
                    debug!("Writing memory {:08x} -> {:08x}", addr, values[0]);
                    cpu.write_memory(bridge, addr, 4, values[0])?;
                } else {
                    for (offset, value) in values.iter().enumerate() {
                        debug!("Writing memory {:08x} -> {:08x}", addr, values[offset]);
                        cpu.write_memory(bridge, addr + (offset as u32 * 4), 4, *value)?;
                    }
                }
                self.gdb_send(b"OK")?
            }
            GdbCommand::VContQuery => self.gdb_send(b"vCont;c;C;s;S")?,
            GdbCommand::VContContinue => {
                if let Some(s) = cpu.resume(bridge)? {
                    self.print_string(&format!("Note: CPU is currently in a trap: {}\n", s))?
                }
            }
            GdbCommand::VContContinueFromSignal(_) => {
                if let Some(s) = cpu.resume(bridge)? {
                    self.print_string(&format!("Note: CPU is currently in a trap: {}\n", s))?
                }
            }
            GdbCommand::VContStepFromSignal(_) => {
                if let Some(s) = cpu.step(bridge)? {
                    self.print_string(&format!("Note: CPU is currently in a trap: {}\n", s))?;
                }
                self.last_signal = 5;
                self.gdb_send(format!("S{:02x}", self.last_signal).as_bytes())?;
            }
            GdbCommand::GetOffsets => self.gdb_send(b"Text=0;Data=0;Bss=0")?,
            GdbCommand::Continue => {
                if let Some(s) = cpu.resume(bridge)? {
                    self.print_string(&format!("Note: CPU is currently in a trap: {}\n", s))?
                }
            }
            GdbCommand::Step => {
                if let Some(s) = cpu.step(bridge)? {
                    self.print_string(&format!("Note: CPU is currently in a trap: {}\n", s))?
                }
            }
            GdbCommand::MonitorCommand(cmd) => {
                match cmd.as_str() {
                    "reset" => {
                        self.print_string("Resetting CPU...\n")?;
                        cpu.reset(&bridge)?;
                    }
                    "about" => {
                        self.print_string("VexRiscv GDB bridge\n")?;
                    }
                    "explain" => {
                        self.print_string(&cpu.explain(&bridge)?)?;
                    }
                    _ => {
                        self.print_string("Unrecognized monitor command.  Available commands:\n")?;
                        self.print_string("    about           - Information about the bridge\n")?;
                        self.print_string("    explain         - Explain what the CPU is doing\n")?;
                        self.print_string("    reset           - Reset the CPU\n")?;
                    }
                }
                self.gdb_send(b"OK")?
            }
            GdbCommand::ReadFeature(filename, offset, len) => {
                self.gdb_send_file(cpu.get_feature(&filename)?, offset, len)?
            }
            GdbCommand::ReadMemoryMap(_offset, _len) => {
                // self.gdb_send_file(cpu.get_memory_map()?, offset, len)?
                self.gdb_send(b"")?
            }
            GdbCommand::ReadThreads(offset, len) => {
                self.gdb_send_file(cpu.get_threads()?, offset, len)?
            }
            GdbCommand::Interrupt => {
                self.last_signal = 2;
                cpu.halt(bridge)?;
                self.gdb_send(format!("S{:02x}", self.last_signal).as_bytes())?;
            }
            GdbCommand::MustReplyEmpty => self.gdb_send(b"")?,
            GdbCommand::Unknown(_) => self.gdb_send(b"")?,
        };
        Ok(())
    }

    fn gdb_send_ack(&mut self) -> io::Result<usize> {
        self.connection.write(&[b'+'])
    }

    fn gdb_send_nak(&mut self) -> io::Result<usize> {
        self.connection.write(&[b'-'])
    }

    fn gdb_send_u32(&mut self, vals: Vec<u32>) -> io::Result<()> {
        let mut out_str = String::new();
        for val in vals {
            let mut buf = [0; 4];
            BigEndian::write_u32(&mut buf, val);
            out_str.push_str(&format!("{:08x}", NativeEndian::read_u32(&buf)));
        }
        self.gdb_send(out_str.as_bytes())
    }

    fn gdb_send(&mut self, inp: &[u8]) -> io::Result<()> {
        let mut buffer = [0; 16388];
        let mut checksum: u8 = 0;
        buffer[0] = b'$';
        for i in 0..inp.len() {
            buffer[i + 1] = inp[i];
            checksum = checksum.wrapping_add(inp[i]);
        }
        let checksum_str = &format!("{:02x}", checksum);
        let checksum_bytes = checksum_str.as_bytes();
        buffer[inp.len() + 1] = b'#';
        buffer[inp.len() + 2] = checksum_bytes[0];
        buffer[inp.len() + 3] = checksum_bytes[1];
        let (to_write, _rest) = buffer.split_at(inp.len() + 4);
        // debug!(
        //     " > Writing {} bytes: {}",
        //     to_write.len(),
        //     String::from_utf8_lossy(&to_write)
        // );
        self.connection.write_all(&to_write)?;
        Ok(())
    }

    pub fn print_string(&mut self, msg: &str) -> io::Result<()> {
        debug!("Printing string {} to GDB", msg);
        let mut strs: Vec<String> = msg
            .as_bytes()
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect();
        strs.insert(0, "O".to_string());
        let joined = strs.join("");
        self.gdb_send(joined.as_bytes())
    }

    fn gdb_send_file(&mut self, mut data: Vec<u8>, offset: u32, len: u32) -> io::Result<()> {
        let offset = offset as usize;
        let len = len as usize;
        let mut end = offset + len;
        if offset > data.len() {
            self.gdb_send(b"l")?;
        } else {
            if end > data.len() {
                end = data.len();
            }
            let mut trimmed_data: Vec<u8> = data.drain(offset..end).collect();
            if trimmed_data.len() >= len {
                // XXX should this be <= or < ?
                trimmed_data.insert(0, b'm');
            } else {
                trimmed_data.insert(0, b'l');
            }
            self.gdb_send(&trimmed_data)?;
        }
        Ok(())
    }
}

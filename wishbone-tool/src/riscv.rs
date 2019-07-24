use super::bridge::{Bridge, BridgeError};
use super::gdb::GdbController;

use log::debug;
use std::cell::{Cell, RefCell};

use std::io;
use std::sync::{Arc, Mutex};
bitflags! {
    struct VexRiscvFlags: u32 {
        const RESET = 1 << 0;
        const HALT = 1 << 1;
        const PIP_BUSY = 1 << 2;
        const HALTED_BY_BREAK = 1 << 3;
        const STEP = 1 << 4;
        const RESET_SET = 1 << 16;
        const HALT_SET = 1 << 17;
        const RESET_CLEAR = 1 << 24;
        const HALT_CLEAR = 1 << 25;
    }
}

fn swab(src: u32) -> u32 {
    (src << 24) & 0xff000000
        | (src << 8) & 0x00ff0000
        | (src >> 8) & 0x0000ff00
        | (src >> 24) & 0x000000ff
}

#[derive(Debug, PartialEq)]
pub enum RiscvCpuState {
    Unknown,
    Halted,
    Running,
}

#[derive(Debug)]
pub enum RiscvCpuError {
    /// Someone tried to request an unrecognized feature file
    UnrecognizedFile(String /* requested filename */),

    /// The given register could not be decoded
    InvalidRegister(u32),

    /// Ran out of breakpoionts
    BreakpointExhausted,

    /// Couldn't find that breakpoint
    BreakpointNotFound(u32 /* address */),

    /// An error occurred with the bridge
    BridgeError(BridgeError),

    /// Generic IO error
    IoError(io::Error),
}

impl std::convert::From<BridgeError> for RiscvCpuError {
    fn from(e: BridgeError) -> RiscvCpuError {
        RiscvCpuError::BridgeError(e)
    }
}

impl std::convert::From<io::Error> for RiscvCpuError {
    fn from(e: io::Error) -> Self {
        RiscvCpuError::IoError(e)
    }
}

const THREADS_XML: &str = r#"<?xml version="1.0"?>
<threads>
</threads>"#;

#[derive(PartialEq)]
enum RiscvRegisterType {
    /// Normal CPU registers
    General,

    /// Arch-specific registers
    CSR,
}

impl RiscvRegisterType {
    fn feature_name(&self) -> &str {
        match *self {
            RiscvRegisterType::General => "org.gnu.gdb.riscv.cpu",
            RiscvRegisterType::CSR => "org.gnu.gdb.riscv.csr",
        }
    }

    fn group(&self) -> &str {
        match *self {
            RiscvRegisterType::General => "general",
            RiscvRegisterType::CSR => "csr",
        }
    }
}

enum RegisterContentsType {
    Int,
    DataPtr,
    CodePtr,
}

struct RiscvRegister {
    /// Which register group this belongs to
    register_type: RiscvRegisterType,

    /// Index within its namespace (e.g. `ustatus` is a CSR with index 0x000,
    /// even though GDB registers are offset by 65, so GDB calls `ustatus` register 65.)
    index: u32,

    /// The "index" as understood by gdb.
    gdb_index: u32,

    /// Architecture name
    name: String,

    /// Whether this register is present on this device
    present: bool,

    /// Whether GDB needs to save and restore it
    save_restore: bool,

    /// What kind of data this register contains
    contents: RegisterContentsType,
}

impl RiscvRegister {
    pub fn general(
        index: u32,
        name: &str,
        save_restore: bool,
        contents: RegisterContentsType,
    ) -> RiscvRegister {
        RiscvRegister {
            register_type: RiscvRegisterType::General,
            index,
            gdb_index: index,
            name: name.to_string(),
            present: true,
            save_restore,
            contents,
        }
    }

    pub fn csr(index: u32, name: &str, present: bool) -> RiscvRegister {
        RiscvRegister {
            register_type: RiscvRegisterType::CSR,
            index,
            gdb_index: index + 65,
            name: name.to_string(),
            present,
            save_restore: true,
            contents: RegisterContentsType::Int,
        }
    }
}

struct RiscvBreakpoint {

    /// The address of the breakpoint
    address: u32,

    /// Whether this breakpoint is enabled
    enabled: bool,

    /// Whether this value is empty or not
    allocated: bool,
}

pub struct RiscvCpu {
    /// A list of all available registers on this CPU
    registers: Vec<RiscvRegister>,

    /// An XML representation of the register mapping
    target_xml: String,

    /// The memory offset of the debug register
    debug_offset: u32,

    /// We'll use $x1 as an accumulator sometimes, so save its value here.
    x1_value: Cell<Option<u32>>,

    /// $x2 sometimes gets used during debug.  Back up its value here
    x2_value: Cell<Option<u32>>,

    /// $pc needs to get refreshed when we hit a breakpoint
    pc_value: Arc<Mutex<Option<u32>>>,

    /// All available breakpoints
    breakpoints: RefCell<[RiscvBreakpoint; 4]>,

    /// CPU state
    cpu_state: Arc<Mutex<RiscvCpuState>>,
}

pub struct RiscvCpuController {
    /// The bridge offset for the debug register
    debug_offset: u32,

    /// A copy of the CPU's state object
    cpu_state: Arc<Mutex<RiscvCpuState>>,

    /// $pc needs to get refreshed when we hit a breakpoint
    pc_value: Arc<Mutex<Option<u32>>>,
}

impl RiscvCpu {
    pub fn new() -> Result<RiscvCpu, RiscvCpuError> {
        let registers = Self::make_registers();
        let target_xml = Self::make_target_xml(&registers);
        Ok(RiscvCpu {
            registers,
            target_xml,
            debug_offset: 0xf00f0000,
            x1_value: Cell::new(None),
            x2_value: Cell::new(None),
            pc_value: Arc::new(Mutex::new(None)),
            breakpoints: RefCell::new([
                RiscvBreakpoint {
                    address: 0,
                    enabled: false,
                    allocated: false,
                },
                RiscvBreakpoint {
                    address: 0,
                    enabled: false,
                    allocated: false,
                },
                RiscvBreakpoint {
                    address: 0,
                    enabled: false,
                    allocated: false,
                },
                RiscvBreakpoint {
                    address: 0,
                    enabled: false,
                    allocated: false,
                },
            ]),
            cpu_state: Arc::new(Mutex::new(RiscvCpuState::Unknown)),
        })
    }

    fn make_registers() -> Vec<RiscvRegister> {
        let mut registers = vec![];

        // Add in general purpose registers x0 to x31
        for reg_num in 0..32 {
            let contents_type = match reg_num {
                2 => RegisterContentsType::DataPtr,
                _ => RegisterContentsType::Int,
            };
            registers.push(RiscvRegister::general(
                reg_num,
                &format!("x{}", reg_num),
                true,
                contents_type,
            ));
        }

        // Add the program counter
        registers.push(RiscvRegister::general(
            32,
            "pc",
            true,
            RegisterContentsType::CodePtr,
        ));

        // User trap setup
        registers.push(RiscvRegister::csr(0x000, "ustatus", false));
        registers.push(RiscvRegister::csr(0x004, "uie", false));
        registers.push(RiscvRegister::csr(0x005, "utvec", false));

        // User trap handling
        registers.push(RiscvRegister::csr(0x040, "uscratch", false));
        registers.push(RiscvRegister::csr(0x041, "uepc", false));
        registers.push(RiscvRegister::csr(0x042, "ucause", false));
        registers.push(RiscvRegister::csr(0x043, "utval", false));
        registers.push(RiscvRegister::csr(0x044, "uip", false));

        // User counter/timers
        registers.push(RiscvRegister::csr(0xc00, "cycle", false));
        registers.push(RiscvRegister::csr(0xc01, "time", false));
        registers.push(RiscvRegister::csr(0xc02, "instret", false));
        for hpmcounter_n in 3..32 {
            registers.push(RiscvRegister::csr(
                0xc00 + hpmcounter_n,
                &format!("hpmcounter{}", hpmcounter_n),
                false,
            ));
        }
        registers.push(RiscvRegister::csr(0xc80, "cycleh", false));
        registers.push(RiscvRegister::csr(0xc81, "timeh", false));
        registers.push(RiscvRegister::csr(0xc82, "instreth", false));
        for hpmcounter_n in 3..32 {
            registers.push(RiscvRegister::csr(
                0xc80 + hpmcounter_n,
                &format!("hpmcounter{}h", hpmcounter_n),
                false,
            ));
        }

        // Supervisor Trap Setup
        registers.push(RiscvRegister::csr(0x100, "sstatus", false));
        registers.push(RiscvRegister::csr(0x102, "sedeleg", false));
        registers.push(RiscvRegister::csr(0x103, "sideleg", false));
        registers.push(RiscvRegister::csr(0x104, "sie", false));
        registers.push(RiscvRegister::csr(0x105, "stvec", false));
        registers.push(RiscvRegister::csr(0x106, "scounteren", false));

        // Supervisor Trap Handling
        registers.push(RiscvRegister::csr(0x140, "sscratch", false));
        registers.push(RiscvRegister::csr(0x141, "sepc", false));
        registers.push(RiscvRegister::csr(0x142, "scause", false));
        registers.push(RiscvRegister::csr(0x143, "stval", false));
        registers.push(RiscvRegister::csr(0x144, "sip", false));

        // Supervisor protection and translation
        registers.push(RiscvRegister::csr(0x180, "satp", false));

        // Machine information registers
        registers.push(RiscvRegister::csr(0xf11, "mvendorid", true));
        registers.push(RiscvRegister::csr(0xf12, "marchid", true));
        registers.push(RiscvRegister::csr(0xf13, "mimpid", true));
        registers.push(RiscvRegister::csr(0xf14, "mhartid", true));

        // Machine trap setup
        registers.push(RiscvRegister::csr(0x300, "mstatus", true));
        registers.push(RiscvRegister::csr(0x301, "misa", false));
        registers.push(RiscvRegister::csr(0x302, "medeleg", false));
        registers.push(RiscvRegister::csr(0x303, "mideleg", false));
        registers.push(RiscvRegister::csr(0x304, "mie", true));
        registers.push(RiscvRegister::csr(0x305, "mtvec", true));
        registers.push(RiscvRegister::csr(0x306, "mcounteren", false));

        // Machine trap handling
        registers.push(RiscvRegister::csr(0x340, "mscratch", true));
        registers.push(RiscvRegister::csr(0x341, "mepc", true));
        registers.push(RiscvRegister::csr(0x342, "mcause", true));
        registers.push(RiscvRegister::csr(0x343, "mtval", true));
        registers.push(RiscvRegister::csr(0x344, "mip", true));

        // Machine protection and translation
        registers.push(RiscvRegister::csr(0x3a0, "mpmcfg0", false));
        registers.push(RiscvRegister::csr(0x3a1, "mpmcfg1", false));
        registers.push(RiscvRegister::csr(0x3a2, "mpmcfg2", false));
        registers.push(RiscvRegister::csr(0x3a3, "mpmcfg3", false));
        for pmpaddr_n in 0..16 {
            registers.push(RiscvRegister::csr(
                0x3b0 + pmpaddr_n,
                &format!("pmpaddr{}", pmpaddr_n),
                false,
            ));
        }

        // Machine counter/timers
        registers.push(RiscvRegister::csr(0xb00, "mcycle", true));
        registers.push(RiscvRegister::csr(0xb02, "minstret", true));
        for mhpmcounter_n in 3..32 {
            registers.push(RiscvRegister::csr(
                0xb00 + mhpmcounter_n,
                &format!("mhpmcounter{}", mhpmcounter_n),
                false,
            ));
        }
        registers.push(RiscvRegister::csr(0xb80, "mcycleh", true));
        registers.push(RiscvRegister::csr(0xb82, "minstreth", true));
        for mhpmcounter_n in 3..32 {
            registers.push(RiscvRegister::csr(
                0xb80 + mhpmcounter_n,
                &format!("mhpmcounter{}h", mhpmcounter_n),
                false,
            ));
        }

        // Machine counter setup
        for mhpmevent_n in 3..32 {
            registers.push(RiscvRegister::csr(
                0x320 + mhpmevent_n,
                &format!("mhpmevent{}", mhpmevent_n),
                false,
            ));
        }

        // Debug/trace registers
        registers.push(RiscvRegister::csr(0x7a0, "tselect", false));
        registers.push(RiscvRegister::csr(0x7a1, "tdata1", false));
        registers.push(RiscvRegister::csr(0x7a2, "tdata2", false));
        registers.push(RiscvRegister::csr(0x7a3, "tdata3", false));

        // Debug mode registers
        registers.push(RiscvRegister::csr(0x7b0, "dcsr", false));
        registers.push(RiscvRegister::csr(0x7b1, "dpc", false));
        registers.push(RiscvRegister::csr(0x7b2, "dscratch", false));

        registers
    }

    fn make_target_xml(registers: &Vec<RiscvRegister>) -> String {
        let mut target_xml = "<?xml version=\"1.0\"?>\n<!DOCTYPE target SYSTEM \"gdb-target.dtd\">\n<target version=\"1.0\">\n".to_string();

        // Add in general-purpose registers
        for ft in &[RiscvRegisterType::General, RiscvRegisterType::CSR] {
            target_xml.push_str(&format!("<feature name=\"{}\">\n", ft.feature_name()));
            for reg in registers {
                if !reg.present || reg.register_type != *ft {
                    continue;
                }
                let reg_type = match reg.contents {
                    RegisterContentsType::Int => "int",
                    RegisterContentsType::CodePtr => "code_ptr",
                    RegisterContentsType::DataPtr => "data_ptr",
                };
                target_xml.push_str(&format!(
                    "<reg name=\"{}\" bitsize=\"32\" regnum=\"{}\" type=\"{}\" group=\"{}\"",
                    reg.name,
                    reg.gdb_index,
                    reg_type,
                    reg.register_type.group()
                ));
                if !reg.save_restore {
                    target_xml.push_str(" save-restore=\"no\"");
                }
                target_xml.push_str("/>\n");
            }
            target_xml.push_str("</feature>\n");
        }
        target_xml.push_str("</target>\n");

        target_xml
    }

    pub fn get_feature(&self, name: &str) -> Result<Vec<u8>, RiscvCpuError> {
        if name == "target.xml" {
            let xml = self.target_xml.to_string().into_bytes();
            Ok(xml)
        } else {
            Err(RiscvCpuError::UnrecognizedFile(name.to_string()))
        }
    }

    pub fn get_threads(&self) -> Result<Vec<u8>, RiscvCpuError> {
        Ok(THREADS_XML.to_string().into_bytes())
    }

    pub fn read_memory(&self, bridge: &Bridge, addr: u32, sz: u32) -> Result<u32, RiscvCpuError> {
        if sz == 4 {
            return Ok(bridge.peek(addr)?);
        }

        // We clobber $x1 in this function, so read its previous value
        // (if we haven't already).
        // This will get restored when we do a reset.
        if self.x1_value.get().is_none() {
            self.x1_value.set(Some(self.read_register(bridge, 1)?));
        }
        // let addr = swab(addr);
        self.write_register(bridge, 1, addr)?;
        let inst = match sz {
            // LW x1, 0(x1)
            4 => (1 << 15) | (0x2 << 12) | (1 << 7) | 0x3,

            // LHU x1, 0(x1)
            2 => (1 << 15) | (0x5 << 12) | (1 << 7) | 0x3,

            // LBU x1, 0(x1)
            1 => (1 << 15) | (0x4 << 12) | (1 << 7) | 0x3,

            x => panic!("Unrecognized memory size: {}", x),
        };
        self.write_instruction(bridge, inst)?;
        Ok(self.read_result(bridge)?)
    }

    pub fn write_memory(
        &self,
        bridge: &Bridge,
        addr: u32,
        sz: u32,
        value: u32,
    ) -> Result<(), RiscvCpuError> {
        if sz == 4 {
            return Ok(bridge.poke(addr, value)?);
        }

        // We clobber $x1 in this function, so read its previous value
        // (if we haven't already).
        // This will get restored when we do a reset.
        if self.x1_value.get().is_none() {
            self.x1_value.set(Some(self.read_register(bridge, 1)?));
        }
        if self.x2_value.get().is_none() {
            self.x2_value.set(Some(self.read_register(bridge, 1)?));
        }

        // let addr = swab(addr);
        self.write_register(bridge, 1, value)?;
        self.write_register(bridge, 2, addr)?;
        let inst = match sz {
            // SW x1,0(x2)
            4 => (1 << 20) | (2 << 15) | (0x2 << 12) | 0x23,

            // SH x1,0(x2)
            2 => (1 << 20) | (2 << 15) | (0x1 << 12) | 0x23,

            //SB x1,0(x2)
            1 => (1 << 20) | (2 << 15) | (0x0 << 12) | 0x23,

            x => panic!("Unrecognized memory size: {}", x),
        };
        self.write_instruction(bridge, inst)?;
        Ok(())
    }

    pub fn add_breakpoint(&self, bridge: &Bridge, addr: u32) -> Result<(), RiscvCpuError> {
        let mut bp_index = None;
        let mut bps = self.breakpoints.borrow_mut();
        for (bpidx, bp) in bps.iter().enumerate() {
            if !bp.allocated {
                bp_index = Some(bpidx);
            }
        }
        if bp_index.is_none() {
            return Err(RiscvCpuError::BreakpointExhausted);
        }

        let bp_index = bp_index.unwrap();

        bps[bp_index].address = addr;
        bps[bp_index].allocated = true;
        bps[bp_index].enabled = true;

        bridge.poke(self.debug_offset + 0x40 + (bp_index as u32 * 4), addr | 1)?;
        Ok(())
    }

    pub fn remove_breakpoint(&self, bridge: &Bridge, addr: u32) -> Result<(), RiscvCpuError> {
        let mut bp_index = None;
        let mut bps = self.breakpoints.borrow_mut();
        for (bpidx, bp) in bps.iter().enumerate() {
            if bp.allocated && bp.address == addr {
                bp_index = Some(bpidx);
            }
        }
        if bp_index.is_none() {
            return Err(RiscvCpuError::BreakpointNotFound(addr));
        }

        let bp_index = bp_index.unwrap();

        bps[bp_index].allocated = false;
        bps[bp_index].enabled = false;

        bridge.poke(self.debug_offset + 0x40 + (bp_index as u32 * 4), 0)?;
        Ok(())
    }

    pub fn halt(&self, bridge: &Bridge) -> Result<(), RiscvCpuError> {
        self.write_status(bridge, VexRiscvFlags::HALT_SET)?;
        *self.cpu_state.lock().unwrap() = RiscvCpuState::Halted;
        self.flush_cache(bridge)?;
        debug!("HALT: CPU is now halted");
        Ok(())
    }

    fn update_breakpoints(&self, bridge: &Bridge) -> Result<(), RiscvCpuError> {
        for (bpidx, bp) in self.breakpoints.borrow().iter().enumerate() {
            if bp.allocated && bp.enabled {
                bridge.poke(
                    self.debug_offset + 0x40 + (bpidx as u32 * 4),
                    bp.address | 1,
                )?;
            } else {
                // If this breakpoint is unallocated, ensure that there is no
                // garbage 
                bridge.poke(
                    self.debug_offset + 0x40 + (bpidx as u32 * 4),
                    0,
                )?;
            }
        }
        Ok(())
    }

    pub fn reset(&self, bridge: &Bridge) -> Result<(), RiscvCpuError> {
        self.update_breakpoints(bridge)?;
        self.flush_cache(bridge)?;
        self.write_status(bridge, VexRiscvFlags::HALT_SET | VexRiscvFlags::RESET_SET)?;
        self.write_status(bridge, VexRiscvFlags::RESET_CLEAR)?;
        *self.cpu_state.lock().unwrap() = RiscvCpuState::Halted;
        debug!("RESET: CPU is now halted and reset");
        Ok(())
    }

    fn restore(&self, bridge: &Bridge) -> Result<(), RiscvCpuError> {
        if let Some(old_value) = self.x1_value.get().take() {
            debug!("Updating old value of x1 to {:08x}", old_value);
            self.write_register(bridge, 1, old_value)?;
        }

        if let Some(old_value) = self.x2_value.get().take() {
            debug!("Updating old value of x2 to {:08x}", old_value);
            self.write_register(bridge, 2, old_value)?;
        }

        if let Some(old_value) = self.pc_value.lock().unwrap().take() {
            debug!("Updating pc to {:08x}", old_value);
            self.write_register(bridge, 32, old_value)?;
        }

        self.flush_cache(bridge)
    }

    pub fn resume(&self, bridge: &Bridge) -> Result<(), RiscvCpuError> {
        self.restore(bridge)?;

        // Rewrite breakpoints (is this necessary?)
        self.update_breakpoints(bridge)?;

        self.write_status(bridge, VexRiscvFlags::HALT_CLEAR)?;
        *self.cpu_state.lock().unwrap() = RiscvCpuState::Running;
        debug!("RESUME: CPU is now running");
        Ok(())
    }

    pub fn step(&self, bridge: &Bridge) -> Result<(), RiscvCpuError> {
        self.restore(bridge)?;
        self.write_status(bridge, VexRiscvFlags::HALT_CLEAR | VexRiscvFlags::STEP)
    }

    /* --- */
    fn get_register(&self, regnum: u32) -> Option<&RiscvRegister> {
        for reg in &self.registers {
            if reg.gdb_index == regnum {
                return Some(&reg);
            }
        }
        None
    }

    pub fn read_register(&self, bridge: &Bridge, regnum: u32) -> Result<u32, RiscvCpuError> {
        let reg = match self.get_register(regnum) {
            None => return Err(RiscvCpuError::InvalidRegister(regnum)),
            Some(s) => s,
        };

        match reg.register_type {
            RiscvRegisterType::General => {
                if reg.index == 32 {
                    self.write_instruction(bridge, 0x17) //AUIPC x0,0
                } else if reg.index == 1 && self.x1_value.get().is_some() {
                    return Ok(self.x1_value.get().unwrap());
                } else if reg.index == 2 && self.x2_value.get().is_some() {
                    return Ok(self.x2_value.get().unwrap());
                } else {
                    self.write_instruction(bridge, (reg.index << 15) | 0x13) //ADDI x0, x?, 0
                }
            }
            RiscvRegisterType::CSR => {
                // We clobber $x1 in this function, so read its previous value
                // (if we haven't already).
                // This will get restored when we do a reset.
                if self.x1_value.get().is_none() {
                    self.x1_value.set(Some(self.read_register(bridge, 1)?));
                }

                // Perform a CSRRW which does a Read/Write.  If rs1 is $x0, then the write
                // is ignored and side-effect free.  Set rd to $x1 to make the read
                // not side-effect free.
                self.write_instruction(
                    bridge,
                    0
                    | ((reg.index & 0x1fff) << 20)
                    | (0 << 15)	    // rs1: x0
                    | (2 << 12)	    // CSRRW
                    | (1 << 7)	    // rd: x1
                    | (0x73 << 0), // SYSTEM
                )
            }
        }?;
        let result = self.read_result(bridge)?;
        debug!("Register x{} value: 0x{:08x}", reg.index, result);
        Ok(result)
    }

    pub fn write_register(
        &self,
        bridge: &Bridge,
        regnum: u32,
        value: u32,
    ) -> Result<(), RiscvCpuError> {
        let reg = match self.get_register(regnum) {
            None => return Err(RiscvCpuError::InvalidRegister(regnum)),
            Some(s) => s,
        };
        if reg.register_type == RiscvRegisterType::General {
            if reg.index == 1 {
                self.x1_value.set(Some(value));
                return Ok(());
            }
            if reg.index == 2 {
                self.x2_value.set(Some(value));
                return Ok(());
            }
            if reg.index == 32 {
                *self.pc_value.lock().unwrap() = Some(value);
                return Ok(());
            }
        }
        self.do_write_register(bridge, regnum, value)
    }

    fn do_write_register(
        &self,
        bridge: &Bridge,
        regnum: u32,
        value: u32,
    ) -> Result<(), RiscvCpuError> {
        let reg = match self.get_register(regnum) {
            None => return Err(RiscvCpuError::InvalidRegister(regnum)),
            Some(s) => s,
        };

        debug!("Setting register x{} -> {:08x}", regnum, value);
        match reg.register_type {
            RiscvRegisterType::General => {
                // Handle PC separately
                if regnum == 32 {
                    self.do_write_register(bridge, 1, value)?;
                    // JALR x1
                    self.write_instruction(bridge, 0x67 | (1 << 15))
                // Use LUI instruction if necessary
                } else if (value & 0xffff_f800) != 0 {
                    let low = value & 0x0000_0fff;
                    let high = if (low & 0x800) != 0 {
                        (value & 0xffff_f000).wrapping_add(0x1000)
                    } else {
                        value & 0xffff_f000
                    };

                    // LUI regId, high
                    self.write_instruction(bridge, (reg.index << 7) | high | 0x37)?;

                    // Also issue ADDI
                    if low != 0 {
                        // ADDI regId, regId, low
                        self.write_instruction(
                            bridge,
                            (reg.index << 7) | (reg.index << 15) | (low << 20) | 0x13,
                        )?;
                    }
                    Ok(())
                } else {
                    // ORI regId, x0, value
                    self.write_instruction(
                        bridge,
                        (reg.index << 7) | (6 << 12) | (value << 20) | 0x13,
                    )
                }
            }
            RiscvRegisterType::CSR => {
                // We clobber $x1 in this function, so read its previous value
                // (if we haven't already).
                // This will get restored when we do a reset.
                if self.x1_value.get().is_none() {
                    self.x1_value.set(Some(self.read_register(bridge, 1)?));
                }

                // Perform a CSRRW which does a Read/Write.  If rd is $x0, then the read
                // is ignored and side-effect free.  Set rs1 to $x1 to make the write
                // not side-effect free.
                //
                // cccc cccc cccc ssss s fff ddddd ooooooo
                // c: CSR number
                // s: rs1 (source register)
                // f: Function
                // d: rd (destination register)
                // o: opcode - 0x73
                self.write_register(bridge, 1, value)?;
                self.write_instruction(
                    bridge,
                    0
                    | ((reg.index & 0x1fff) << 20)
                    | (1 << 15)	    // rs1: x1
                    | (1 << 12)	    // CSRRW
                    | (0 << 7)	    // rd: x0
                    | (0x73 << 0), // SYSTEM
                )
            }
        }
    }

    /* --- */
    fn write_status(&self, bridge: &Bridge, value: VexRiscvFlags) -> Result<(), RiscvCpuError> {
        debug!("SETTING BRIDGE STATUS: {:08x}", value.bits);
        bridge.poke(self.debug_offset, value.bits)?;
        Ok(())
    }

    fn read_status(&self, bridge: &Bridge) -> Result<VexRiscvFlags, RiscvCpuError> {
        match bridge.peek(self.debug_offset) {
            Err(e) => Err(RiscvCpuError::BridgeError(e)),
            Ok(bits) => Ok(VexRiscvFlags { bits }),
        }
    }

    fn write_instruction(&self, bridge: &Bridge, opcode: u32) -> Result<(), RiscvCpuError> {
        debug!(
            "WRITE INSTRUCTION: 0x{:08x} -- 0x{:08x}",
            opcode,
            swab(opcode)
        );
        bridge.poke(self.debug_offset + 4, opcode)?;
        loop {
            if (self.read_status(bridge)? & VexRiscvFlags::PIP_BUSY) != VexRiscvFlags::PIP_BUSY {
                break;
            }
        }
        Ok(())
    }

    fn read_result(&self, bridge: &Bridge) -> Result<u32, RiscvCpuError> {
        Ok(bridge.peek(self.debug_offset + 4)?)
    }

    pub fn get_controller(&self) -> RiscvCpuController {
        RiscvCpuController {
            cpu_state: self.cpu_state.clone(),
            debug_offset: self.debug_offset,
            pc_value: Arc::new(Mutex::new(None)),
        }
    }

    pub fn flush_cache(&self, bridge: &Bridge) -> Result<(), RiscvCpuError> {
        for opcode in vec![4111, 19, 19, 19] {
            self.write_instruction(bridge, opcode)?;
        }
        Ok(())
    }
}

fn is_running(flags: VexRiscvFlags) -> bool {
    // debug!("CPU flags: {:?}", flags);
    ((flags & VexRiscvFlags::PIP_BUSY) == VexRiscvFlags::PIP_BUSY)
        || ((flags & VexRiscvFlags::HALT) != VexRiscvFlags::HALT)
}

impl RiscvCpuController {
    pub fn poll(
        &self,
        bridge: &Bridge,
        gdb_controller: &mut GdbController,
    ) -> Result<(), RiscvCpuError> {
        let flags = self.read_status(bridge)?;
        let mut current_status = self.cpu_state.lock().unwrap();
        if !is_running(flags) {
            // debug!("POLL: CPU seems running?");
            if *current_status == RiscvCpuState::Running {
                *current_status = RiscvCpuState::Halted;
                debug!("POLL: CPU is now halted");
                gdb_controller.gdb_send(b"T05 swbreak:;")?;

                // If we were halted by a breakpoint, save the PC (because it will
                // be unavailable later).
                if flags & VexRiscvFlags::HALTED_BY_BREAK == VexRiscvFlags::HALTED_BY_BREAK {
                    *self.pc_value.lock().unwrap() = Some(self.read_result(bridge)?);
                }
                self.flush_cache(bridge)?;
                // We're halted now
            }
        } else {
            // debug!("POLL: CPU seems halted?");
            if *current_status == RiscvCpuState::Halted {
                *current_status = RiscvCpuState::Running;
                debug!("POLL: CPU is now running");
            }
        }
        Ok(())
    }

    fn read_status(&self, bridge: &Bridge) -> Result<VexRiscvFlags, RiscvCpuError> {
        match bridge.peek(self.debug_offset) {
            Err(e) => Err(RiscvCpuError::BridgeError(e)),
            Ok(bits) => Ok(VexRiscvFlags { bits }),
        }
    }

    fn flush_cache(&self, bridge: &Bridge) -> Result<(), RiscvCpuError> {
        for opcode in vec![4111, 19, 19, 19] {
            self.write_instruction(bridge, opcode)?;
        }
        Ok(())
    }

    fn write_instruction(&self, bridge: &Bridge, opcode: u32) -> Result<(), RiscvCpuError> {
        debug!(
            "WRITE INSTRUCTION: 0x{:08x} -- 0x{:08x}",
            opcode,
            swab(opcode)
        );
        bridge.poke(self.debug_offset + 4, opcode)?;
        loop {
            if (self.read_status(bridge)? & VexRiscvFlags::PIP_BUSY) != VexRiscvFlags::PIP_BUSY {
                break;
            }
        }
        Ok(())
    }

    fn read_result(&self, bridge: &Bridge) -> Result<u32, RiscvCpuError> {
        Ok(bridge.peek(self.debug_offset + 4)?)
    }
}
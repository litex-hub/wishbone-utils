use super::gdb::GdbController;
use wishbone_bridge::{Bridge, BridgeError};

use log::{debug, info};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub mod exception;
use exception::RiscvException;

bitflags! {
    struct VexRiscvFlags: u32 {
        const RESET = 1;
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

// fn swab(src: u32) -> u32 {
//     (src << 24) & 0xff000000
//         | (src << 8) & 0x00ff0000
//         | (src >> 8) & 0x0000ff00
//         | (src >> 24) & 0x000000ff
// }

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

    /// CPU didn't complete write
    InstructionTimeout,
}

impl ::std::fmt::Display for RiscvCpuError {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        use RiscvCpuError::*;
        match self {
            UnrecognizedFile(s) => write!(f, "unrecognized file: {}", s),
            InvalidRegister(r) => write!(f, "invalid register {}", r),
            BreakpointExhausted => write!(f, "ran out of hardware breakpoints"),
            BreakpointNotFound(b) => write!(f, "breakpoint {} not found", b),
            BridgeError(e) => write!(f, "bridge error: {}", e),
            IoError(e) => write!(f, "io error: {}", e),
            InstructionTimeout => write!(f, "cpu instruction timed out"),
        }
    }
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

// const MEMORY_MAP_XML: &str = r#"<?xml version="1.0"?>
// <!DOCTYPE memory-map
//           PUBLIC "+//IDN gnu.org//DTD GDB Memory Map V1.0//EN"
//                  "http://sourceware.org/gdb/gdb-memory-map.dtd">
// <memory-map>
//     <memory type="rom" start="0" length="0x2000"/>
//     <memory type="ram" start="0x10000000" length="0x20000"/>
//     <memory type="ram" start="0xe0000000" length="0x10000000"/>
//     <memory type="flash" start="0x20000000" length="0x200000">
//         <property name="blocksize">0x1000</property>
//     </memory>
// </memory-map>"#;

const THREADS_XML: &str = r#"<?xml version="1.0"?>
<threads>
</threads>"#;

#[derive(Debug, PartialEq, Hash, Eq, Clone)]
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

#[derive(Debug, PartialEq, Hash, Eq, Clone)]
enum RegisterContentsType {
    Int,
    DataPtr,
    CodePtr,
}

#[derive(Debug, PartialEq, Hash, Eq, Clone)]
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
            gdb_index: index + Self::csr_offset(),
            name: name.to_string(),
            present,
            save_restore: true,
            contents: RegisterContentsType::Int,
        }
    }

    fn csr_offset() -> u32 {
        65
    }

    pub fn x0() -> RiscvRegister {
        RiscvRegister::general(0, "x0", false, RegisterContentsType::Int)
    }

    pub fn x1() -> RiscvRegister {
        RiscvRegister::general(1, "x1", false, RegisterContentsType::Int)
    }

    pub fn x2() -> RiscvRegister {
        RiscvRegister::general(2, "x2", false, RegisterContentsType::DataPtr)
    }

    pub fn pc() -> RiscvRegister {
        RiscvRegister::general(32, "pc", false, RegisterContentsType::CodePtr)
    }

    pub fn satp() -> RiscvRegister {
        RiscvRegister::csr(0x180, "satp", true)
    }

    pub fn mstatus() -> RiscvRegister {
        RiscvRegister::csr(0x300, "mstatus", true)
    }

    pub fn mtvec() -> RiscvRegister {
        RiscvRegister::csr(0x305, "mtvec", true)
    }

    pub fn mepc() -> RiscvRegister {
        RiscvRegister::csr(0x341, "mepc", true)
    }

    pub fn mcause() -> RiscvRegister {
        RiscvRegister::csr(0x342, "mcause", true)
    }

    pub fn mtval() -> RiscvRegister {
        RiscvRegister::csr(0x343, "mtval", true)
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
    gdb_register_map: HashMap<u32, RiscvRegister>,

    /// An XML representation of the register mapping
    target_xml: String,

    /// The memory offset of the debug register
    debug_offset: u32,

    /// Keep a copy of values that get clobbered during debugging
    cached_values: Arc<Mutex<HashMap<RiscvRegister, u32>>>,

    /// All available breakpoints
    breakpoints: RefCell<[RiscvBreakpoint; 2]>,

    /// CPU state
    cpu_state: Arc<Mutex<RiscvCpuState>>,

    /// Our own interface to the CPU
    controller: RiscvCpuController,

    /// "true" if an MMU exists on this CPU
    has_mmu: bool,

    /// "true" if the MMU is currently enabled
    mmu_enabled: Arc<AtomicBool>,

    /// The last exception, if any
    last_exception: Arc<Mutex<Option<RiscvException>>>,
}

pub struct RiscvCpuController {
    /// The bridge offset for the debug register
    debug_offset: u32,

    /// A copy of the CPU's state object
    cpu_state: Arc<Mutex<RiscvCpuState>>,

    /// Cached values (mostly the program counter)
    cached_values: Arc<Mutex<HashMap<RiscvRegister, u32>>>,

    /// "true" if an MMU exists on this CPU
    has_mmu: bool,

    /// "true" if the MMU is currently enabled
    mmu_enabled: Arc<AtomicBool>,

    /// The last exception, if any
    last_exception: Arc<Mutex<Option<RiscvException>>>,
}

impl RiscvCpu {
    pub fn new(bridge: &Bridge, offset: u32) -> Result<RiscvCpu, RiscvCpuError> {
        let mut gdb_register_map = Self::make_registers();

        let cpu_state = Arc::new(Mutex::new(RiscvCpuState::Unknown));
        let debug_offset = offset;
        let cached_values = Arc::new(Mutex::new(HashMap::new()));
        let last_exception = Arc::new(Mutex::new(None));

        let mmu_enabled = Arc::new(AtomicBool::new(false));
        let mut controller = RiscvCpuController {
            cpu_state: cpu_state.clone(),
            cached_values: cached_values.clone(),
            debug_offset,
            has_mmu: false,
            mmu_enabled: mmu_enabled.clone(),
            last_exception: last_exception.clone(),
        };

        // Determine if this CPU has an MMU.
        // Read the "satp" register and write the opposite value back in.
        // If the value changes, then we know this register exists.
        let was_running =
            (controller.read_status(bridge)? & VexRiscvFlags::HALT) != VexRiscvFlags::HALT;
        if was_running {
            controller.perform_halt(bridge)?;
        }
        let satp_register = RiscvRegister::satp();
        let old_satp = controller.read_register(bridge, &satp_register)?;
        controller.write_register(bridge, &satp_register, !old_satp)?;
        let new_satp = controller.read_register(bridge, &satp_register)?;
        if new_satp != old_satp {
            controller.write_register(bridge, &satp_register, old_satp)?;
            controller.has_mmu = true;
            Self::insert_register(&mut gdb_register_map, satp_register);
            mmu_enabled.store((old_satp & 0x8000_0000) == 0x8000_0000, Ordering::Relaxed);
        }
        if was_running {
            controller.perform_resume(bridge, false)?;
        }

        let target_xml = Self::make_target_xml(&gdb_register_map);

        let has_mmu = controller.has_mmu;
        let cpu = RiscvCpu {
            gdb_register_map,
            target_xml,
            debug_offset,
            cached_values,
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
                // RiscvBreakpoint {
                //     address: 0,
                //     enabled: false,
                //     allocated: false,
                // },
                // RiscvBreakpoint {
                //     address: 0,
                //     enabled: false,
                //     allocated: false,
                // },
            ]),
            controller,
            cpu_state,
            has_mmu,
            mmu_enabled,
            last_exception,
        };

        Ok(cpu)
    }

    fn insert_register(target: &mut HashMap<u32, RiscvRegister>, reg: RiscvRegister) {
        target.insert(reg.gdb_index, reg);
    }

    fn make_registers() -> HashMap<u32, RiscvRegister> {
        let mut registers = HashMap::new();

        // Add in general purpose registers x0 to x31
        registers.insert(0, RiscvRegister::x0());
        registers.insert(1, RiscvRegister::x1());
        registers.insert(2, RiscvRegister::x2());
        for reg_num in 3..32 {
            registers.insert(
                reg_num,
                RiscvRegister::general(
                    reg_num,
                    &format!("x{}", reg_num),
                    true,
                    RegisterContentsType::Int,
                ),
            );
        }

        // Add the program counter
        registers.insert(32, RiscvRegister::pc());

        // User trap setup
        Self::insert_register(&mut registers, RiscvRegister::csr(0x000, "ustatus", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x004, "uie", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x005, "utvec", false));

        // User trap handling
        Self::insert_register(&mut registers, RiscvRegister::csr(0x040, "uscratch", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x041, "uepc", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x042, "ucause", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x043, "utval", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x044, "uip", false));

        // User counter/timers
        Self::insert_register(&mut registers, RiscvRegister::csr(0xc00, "cycle", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0xc01, "time", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0xc02, "instret", false));
        for hpmcounter_n in 3..32 {
            Self::insert_register(
                &mut registers,
                RiscvRegister::csr(
                    0xc00 + hpmcounter_n,
                    &format!("hpmcounter{}", hpmcounter_n),
                    false,
                ),
            );
        }
        Self::insert_register(&mut registers, RiscvRegister::csr(0xc80, "cycleh", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0xc81, "timeh", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0xc82, "instreth", false));
        for hpmcounter_n in 3..32 {
            Self::insert_register(
                &mut registers,
                RiscvRegister::csr(
                    0xc80 + hpmcounter_n,
                    &format!("hpmcounter{}h", hpmcounter_n),
                    false,
                ),
            );
        }

        // Supervisor Trap Setup
        Self::insert_register(&mut registers, RiscvRegister::csr(0x100, "sstatus", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x102, "sedeleg", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x103, "sideleg", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x104, "sie", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x105, "stvec", false));
        Self::insert_register(
            &mut registers,
            RiscvRegister::csr(0x106, "scounteren", false),
        );

        // Supervisor Trap Handling
        Self::insert_register(&mut registers, RiscvRegister::csr(0x140, "sscratch", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x141, "sepc", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x142, "scause", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x143, "stval", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x144, "sip", false));

        // Supervisor protection and translation
        Self::insert_register(&mut registers, RiscvRegister::csr(0x180, "satp", false));

        // Machine information registers
        Self::insert_register(&mut registers, RiscvRegister::csr(0xf11, "mvendorid", true));
        Self::insert_register(&mut registers, RiscvRegister::csr(0xf12, "marchid", true));
        Self::insert_register(&mut registers, RiscvRegister::csr(0xf13, "mimpid", true));
        Self::insert_register(&mut registers, RiscvRegister::csr(0xf14, "mhartid", true));

        // Machine trap setup
        Self::insert_register(&mut registers, RiscvRegister::mstatus());
        Self::insert_register(&mut registers, RiscvRegister::csr(0x301, "misa", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x302, "medeleg", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x303, "mideleg", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x304, "mie", true));
        Self::insert_register(&mut registers, RiscvRegister::mtvec());
        Self::insert_register(
            &mut registers,
            RiscvRegister::csr(0x306, "mcounteren", false),
        );

        // Machine trap handling
        Self::insert_register(&mut registers, RiscvRegister::csr(0x340, "mscratch", true));
        Self::insert_register(&mut registers, RiscvRegister::mepc());
        Self::insert_register(&mut registers, RiscvRegister::mcause());
        Self::insert_register(&mut registers, RiscvRegister::mtval());
        Self::insert_register(&mut registers, RiscvRegister::csr(0x344, "mip", true));

        // Machine protection and translation
        Self::insert_register(&mut registers, RiscvRegister::csr(0x3a0, "mpmcfg0", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x3a1, "mpmcfg1", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x3a2, "mpmcfg2", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x3a3, "mpmcfg3", false));
        for pmpaddr_n in 0..16 {
            Self::insert_register(
                &mut registers,
                RiscvRegister::csr(0x3b0 + pmpaddr_n, &format!("pmpaddr{}", pmpaddr_n), false),
            );
        }

        // Machine counter/timers
        Self::insert_register(&mut registers, RiscvRegister::csr(0xb00, "mcycle", true));
        Self::insert_register(&mut registers, RiscvRegister::csr(0xb02, "minstret", true));
        for mhpmcounter_n in 3..32 {
            Self::insert_register(
                &mut registers,
                RiscvRegister::csr(
                    0xb00 + mhpmcounter_n,
                    &format!("mhpmcounter{}", mhpmcounter_n),
                    false,
                ),
            );
        }
        Self::insert_register(&mut registers, RiscvRegister::csr(0xb80, "mcycleh", true));
        Self::insert_register(&mut registers, RiscvRegister::csr(0xb82, "minstreth", true));
        for mhpmcounter_n in 3..32 {
            Self::insert_register(
                &mut registers,
                RiscvRegister::csr(
                    0xb80 + mhpmcounter_n,
                    &format!("mhpmcounter{}h", mhpmcounter_n),
                    false,
                ),
            );
        }

        // Machine counter setup
        for mhpmevent_n in 3..32 {
            Self::insert_register(
                &mut registers,
                RiscvRegister::csr(
                    0x320 + mhpmevent_n,
                    &format!("mhpmevent{}", mhpmevent_n),
                    false,
                ),
            );
        }

        // Debug/trace registers
        Self::insert_register(&mut registers, RiscvRegister::csr(0x7a0, "tselect", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x7a1, "tdata1", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x7a2, "tdata2", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x7a3, "tdata3", false));

        // Debug mode registers
        Self::insert_register(&mut registers, RiscvRegister::csr(0x7b0, "dcsr", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x7b1, "dpc", false));
        Self::insert_register(&mut registers, RiscvRegister::csr(0x7b2, "dscratch", false));

        registers
    }

    fn make_target_xml(registers: &HashMap<u32, RiscvRegister>) -> String {
        let mut reg_indexes: Vec<u32> = registers.keys().copied().collect();
        reg_indexes.sort();
        let mut target_xml = "<?xml version=\"1.0\"?>\n<!DOCTYPE target SYSTEM \"gdb-target.dtd\">\n<target version=\"1.0\">\n".to_string();

        let mut last_register_type = None;
        for reg_index in reg_indexes {
            let reg = registers.get(&reg_index).unwrap();
            if Some(&reg.register_type) != last_register_type {
                if last_register_type != None {
                    target_xml.push_str("</feature>\n");
                }
                target_xml.push_str(&format!(
                    "<feature name=\"{}\">\n",
                    reg.register_type.feature_name()
                ));
                last_register_type = Some(&reg.register_type);
            }
            if !reg.present {
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
        if last_register_type != None {
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

    // pub fn get_memory_map(&self) -> Result<Vec<u8>, RiscvCpuError> {
    //     Ok(MEMORY_MAP_XML.to_string().into_bytes())
    // }

    /// Print information about why the CPU got into its current state
    pub fn explain(&self, bridge: &Bridge) -> Result<String, RiscvCpuError> {
        let exception = self.controller.get_current_trap(bridge)?;

        // We assume interrupts are enabled, and if they're disabled it's
        // because we're currently handling one.
        if self.controller.interrupts_enabled(bridge)? {
            Ok(format!("Last trap was: {}\n", exception))
        } else {
            Ok(format!("Current trap is: {}\n", exception))
        }
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
        // let _bridge_mutex = bridge.mutex().lock().unwrap();
        let mut current_status = self.cpu_state.lock().unwrap();
        *current_status = RiscvCpuState::Halted;
        self.controller.perform_halt(bridge)?;
        debug!("HALT: CPU is now halted");
        Ok(())
    }

    fn update_breakpoints(&self, bridge: &Bridge) -> Result<(), RiscvCpuError> {
        for (bpidx, bp) in self.breakpoints.borrow().iter().enumerate() {
            if bp.allocated && bp.enabled {
                debug!(
                    "Re-enabling breakpoint {} at address {:08x}",
                    bpidx, bp.address
                );
                bridge.poke(
                    self.debug_offset + 0x40 + (bpidx as u32 * 4),
                    bp.address | 1,
                )?;
            } else {
                debug!("Breakpoint {} is unallocated", bpidx);
                // If this breakpoint is unallocated, ensure that there is no
                // garbage breakpoints leftover from a previous session.
                bridge.poke(self.debug_offset + 0x40 + (bpidx as u32 * 4), 0)?;
            }
        }
        Ok(())
    }

    /// Reset the target CPU, restore any breakpoints, and leave it in
    /// the "halted" state.
    pub fn reset(&self, bridge: &Bridge) -> Result<(), RiscvCpuError> {
        // let _bridge_mutex = bridge.mutex().lock().unwrap();
        // Since we're resetting the CPU, invalidate all cached registers
        self.cached_values.lock().unwrap().drain();
        self.flush_cache(bridge)?;
        self.mmu_enabled.store(false, Ordering::Relaxed);
        *self.last_exception.lock().unwrap() = None;

        self.controller
            .write_status(bridge, VexRiscvFlags::HALT_SET)?;
        self.controller
            .write_status(bridge, VexRiscvFlags::HALT_SET | VexRiscvFlags::RESET_SET)?;
        self.controller
            .write_status(bridge, VexRiscvFlags::RESET_CLEAR)?;

        *self.cpu_state.lock().unwrap() = RiscvCpuState::Halted;
        debug!("RESET: CPU is now halted and reset");
        Ok(())
    }

    /// Restore the CPU state and continue execution.
    pub fn resume(&self, bridge: &Bridge) -> Result<Option<String>, RiscvCpuError> {
        // let _bridge_mutex = bridge.mutex().lock().unwrap();
        *self.cpu_state.lock().unwrap() = RiscvCpuState::Running;
        // Rewrite breakpoints (is this necessary?)
        self.update_breakpoints(bridge)?;
        self.controller.perform_resume(bridge, false)?;

        if let Some(exception) = self.last_exception.lock().unwrap().take() {
            if exception != RiscvException::NoException {
                return Ok(Some(format!("{}", exception)));
            }
        }
        Ok(None)
    }

    /// Step the CPU forward by one instruction.
    pub fn step(&self, bridge: &Bridge) -> Result<Option<String>, RiscvCpuError> {
        // let _bridge_mutex = bridge.mutex().lock().unwrap();
        self.controller.perform_resume(bridge, true)?;

        if let Some(exception) = self.last_exception.lock().unwrap().take() {
            if exception != RiscvException::NoException {
                return Ok(Some(format!("{}", exception)));
            }
        }
        Ok(None)
    }

    /// Convert a GDB `regnum` into a `RiscvRegister`
    ///
    /// Note that `regnum` is a GDB-based register number, and corresponds
    /// to the `gdb_index` property.
    fn gdb_to_register(&self, regnum: u32) -> Result<&RiscvRegister, RiscvCpuError> {
        match self.gdb_register_map.get(&regnum) {
            Some(s) => Ok(s),
            None => Err(RiscvCpuError::InvalidRegister(regnum)),
        }
    }

    /// Read the specified register and return its value.
    ///
    /// The `gdb_idx` is the GDB index, and may include both CPU registers
    /// and CSR-index registers, which are offset by an index.
    pub fn read_register(&self, bridge: &Bridge, gdb_idx: u32) -> Result<u32, RiscvCpuError> {
        let reg = self.gdb_to_register(gdb_idx)?;

        // Give the cached value, if we have it.
        if let Some(val) = self.get_cached_reg(reg) {
            return Ok(val);
        }

        self.controller.read_register(bridge, reg)
    }

    /// Return a vec containing all valid CPU registers.
    pub fn all_cpu_registers(&self) -> Vec<u32> {
        let mut v = vec![];
        for (idx, reg) in &self.gdb_register_map {
            if reg.register_type == RiscvRegisterType::General {
                v.push(*idx);
            }
        }
        v.sort();
        v
    }

    /// Write a register on the device.
    ///
    /// For general-purpose registers, simply place the new value in the
    /// cache, to be updated when we resume the CPU.
    ///
    /// For CSRs, initiate the write immediately.
    pub fn write_register(
        &self,
        bridge: &Bridge,
        gdb_idx: u32,
        value: u32,
    ) -> Result<(), RiscvCpuError> {
        // let _bridge_mutex = bridge.mutex().lock().unwrap();
        let reg = self.gdb_to_register(gdb_idx)?;
        if reg.register_type == RiscvRegisterType::General {
            self.set_cached_reg(reg, value);
            Ok(())
        } else if reg.gdb_index == RiscvRegister::satp().gdb_index {
            self.mmu_enabled
                .store(value & 0x8000_0000 == 0x8000_0000, Ordering::Relaxed);
            self.set_cached_reg(reg, value);
            Ok(())
        } else {
            self.controller.write_register(bridge, reg, value)
        }
    }

    pub fn read_memory(&self, bridge: &Bridge, addr: u32, sz: u32) -> Result<u32, RiscvCpuError> {
        // let _bridge_mutex = bridge.mutex().lock().unwrap();
        self.controller.read_memory(bridge, addr, sz)
    }

    pub fn write_memory(
        &self,
        bridge: &Bridge,
        addr: u32,
        sz: u32,
        value: u32,
    ) -> Result<(), RiscvCpuError> {
        // let _bridge_mutex = bridge.mutex().lock().unwrap();
        self.controller.write_memory(bridge, addr, sz, value)
    }

    pub fn get_controller(&self) -> RiscvCpuController {
        RiscvCpuController {
            cpu_state: self.cpu_state.clone(),
            debug_offset: self.debug_offset,
            cached_values: self.cached_values.clone(),
            has_mmu: self.has_mmu,
            mmu_enabled: self.mmu_enabled.clone(),
            last_exception: self.last_exception.clone(),
        }
    }

    fn get_cached_reg(&self, reg: &RiscvRegister) -> Option<u32> {
        match self.cached_values.lock().unwrap().get(reg) {
            Some(x) => Some(*x),
            None => None,
        }
    }

    fn set_cached_reg(&self, reg: &RiscvRegister, value: u32) {
        self.cached_values
            .lock()
            .unwrap()
            .insert(reg.clone(), value);
    }

    pub fn flush_cache(&self, bridge: &Bridge) -> Result<(), RiscvCpuError> {
        self.controller.flush_cache(bridge)
    }
}

fn is_running(flags: VexRiscvFlags) -> bool {
    // debug!("CPU flags: {:?}", flags);
    ((flags & VexRiscvFlags::PIP_BUSY) == VexRiscvFlags::PIP_BUSY)
        || ((flags & VexRiscvFlags::HALT) != VexRiscvFlags::HALT)
}

impl RiscvCpuController {
    /// Poll the CPU and determine if it's running or not.  If it
    /// transitions between states, handle this transition as appropriate.
    pub fn poll(
        &self,
        bridge: &Bridge,
        gdb_controller: &mut GdbController,
    ) -> Result<bool, RiscvCpuError> {
        // let _bridge_mutex = bridge.mutex().lock().unwrap();
        let flags = self.read_status(bridge)?;
        let mut current_status = self.cpu_state.lock().unwrap();

        if !is_running(flags) {
            // If the status was running, transition to the `halted` state.
            if *current_status == RiscvCpuState::Running {
                *current_status = RiscvCpuState::Halted;
                // gdb_controller.gdb_send(b"T05swbreak:;")?;

                // If we were halted by a breakpoint, save the PC (because it will
                // be unavailable later).
                let halt_msg =
                    if flags & VexRiscvFlags::HALTED_BY_BREAK == VexRiscvFlags::HALTED_BY_BREAK {
                        // The actual opcode doesn't get executed when halted by a break, but
                        // the pc gets incremented.  Save the target pc so that we can execute it
                        // when we step/resume.
                        let pc = self.read_result(bridge)?;
                        self.cached_values
                            .lock()
                            .unwrap()
                            .insert(RiscvRegister::pc(), pc);
                        "05"
                    } else {
                        "02"
                    };

                self.perform_halt(bridge)?;
                debug!("POLL: CPU is now halted");
                gdb_controller.gdb_send(format!("T{}", halt_msg).as_bytes())?;
            }
        } else {
            // If we're currently running but we shouldn't be, flush caches and stop.
            if *current_status == RiscvCpuState::Halted {
                info!("POLL: The debugger thinks the CPU is halted, but CPU is now running!  Halting it and flushing the caches.");
                self.cached_values.lock().unwrap().drain();
                self.perform_halt(bridge)?;
            }
        }
        Ok(*current_status == RiscvCpuState::Running)
    }

    fn perform_halt(&self, bridge: &Bridge) -> Result<(), RiscvCpuError> {
        self.write_status(bridge, VexRiscvFlags::HALT_SET)?;
        self.flush_cache(bridge)?;

        let mut last_exception = self.last_exception.lock().unwrap();
        let exception = Some(self.get_current_trap(bridge)?);
        if !self.interrupts_enabled(bridge)? && exception != *last_exception {
            *last_exception = exception;
        }

        // VexRiscv can't leave the MMU running when in debug mode.  This is
        // due to how it manipulates the D$.
        // IF the MMU is present and enabled when we enter debug mode,
        // capture the cached value and zero out the register prior to
        // halting the CPU.
        // It will be added to the register cache so that it is restored
        // when the CPU is resumed.
        if self.has_mmu {
            let satp = RiscvRegister::satp();
            let satp_value = self.read_register(bridge, &satp)?;
            self.mmu_enabled
                .store(satp_value & 0x8000_0000 == 0x8000_0000, Ordering::Relaxed);
            if satp_value & 0x8000_0000 == 0x8000_0000 {
                debug!("cpu has an mmu that is enabled -=  disabling it while in debug mode");
                self.set_cached_reg(&satp, satp_value);
                self.write_register(bridge, &satp, satp_value & !0x8000_0000)?;
            }
        }
        Ok(())
    }

    fn perform_resume(&self, bridge: &Bridge, step_only: bool) -> Result<(), RiscvCpuError> {
        let coll: HashMap<RiscvRegister, u32> = {
            let mut cached_registers = self.cached_values.lock().unwrap();
            let drain = cached_registers.drain();
            drain.collect()
        };

        // Do two passes through the list.
        // Register 32 (pc), as well as the CSRs all clobber x1/x2, so
        // update those two values last.
        for (reg, value) in &coll {
            if reg.gdb_index > 2 {
                debug!("restoring value of {} to {:08x}", reg.name, value);
                self.write_register(bridge, reg, *value)?;
            }
        }

        for (reg, value) in coll {
            if reg.gdb_index <= 2 {
                debug!("restoring value of {} to {:08x}", reg.name, value);
                self.write_register(bridge, &reg, value)?;
            }
        }

        self.flush_cache(bridge)?;

        if step_only {
            self.write_status(bridge, VexRiscvFlags::HALT_CLEAR | VexRiscvFlags::STEP)?;
        } else {
            self.write_status(bridge, VexRiscvFlags::HALT_CLEAR)?;
            debug!("RESUME: CPU is now running");
        }
        Ok(())
    }

    pub fn interrupts_enabled(&self, bridge: &Bridge) -> Result<bool, RiscvCpuError> {
        let mstatus_reg = RiscvRegister::mstatus();
        let mstatus = self.read_register(bridge, &mstatus_reg)?;

        if mstatus & (1 << 3) != 0 {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Return the current CPU trap, which could be an interrupt or an
    /// exception.
    pub fn get_current_trap(&self, bridge: &Bridge) -> Result<RiscvException, RiscvCpuError> {
        let mcause_reg = RiscvRegister::mcause();
        let mepc_reg = RiscvRegister::mepc();
        let mtval_reg = RiscvRegister::mtval();

        let mcause = self.read_register(bridge, &mcause_reg)?;
        let mepc = self.read_register(bridge, &mepc_reg)?;
        let mtval = self.read_register(bridge, &mtval_reg)?;

        Ok(RiscvException::from_regs(mcause, mepc, mtval))
    }

    fn read_status(&self, bridge: &Bridge) -> Result<VexRiscvFlags, RiscvCpuError> {
        match bridge.peek(self.debug_offset) {
            Err(e) => Err(RiscvCpuError::BridgeError(e)),
            Ok(bits) => Ok(VexRiscvFlags { bits }),
        }
    }

    fn read_memory(&self, bridge: &Bridge, addr: u32, sz: u32) -> Result<u32, RiscvCpuError> {
        if sz == 4 {
            return Ok(bridge.peek(addr)?);
        } else if sz == 2 {
            return Ok((bridge.peek(addr & !0x3)? >> (8 * (addr & 2))) & 0xffff);
        } else if sz == 1 {
            return Ok((bridge.peek(addr & !0x3)? >> (8 * (addr & 3))) & 0xff);
        }

        // We clobber $x1 in this function, so read its previous value
        // (if we haven't already).
        // This will get restored when we do a reset.
        if self.get_cached_reg(&RiscvRegister::x1()).is_none() {
            self.set_cached_reg(
                &RiscvRegister::x1(),
                self.read_register(bridge, &RiscvRegister::x1())?,
            );
        }

        self.write_register(bridge, &RiscvRegister::x1(), addr)?;
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

    fn write_memory(
        &self,
        bridge: &Bridge,
        addr: u32,
        sz: u32,
        value: u32,
    ) -> Result<(), RiscvCpuError> {
        if sz == 4 {
            return Ok(bridge.poke(addr, value)?);
        }

        // We clobber $x1 and $x2 in this function, so read their previous
        // values (if we haven't already).
        // This will get restored when we do a reset.
        for reg in &[RiscvRegister::x1(), RiscvRegister::x2()] {
            if self.get_cached_reg(&reg).is_none() {
                self.set_cached_reg(&reg, self.read_register(bridge, reg)?);
            }
        }

        self.write_register(bridge, &RiscvRegister::x1(), value)?;
        self.write_register(bridge, &RiscvRegister::x2(), addr)?;
        let inst = match sz {
            // SW x1,0(x2)
            4 => (1 << 20) | (2 << 15) | (0x2 << 12) | 0x23,

            // SH x1,0(x2)
            2 => (1 << 20) | (2 << 15) | (0x1 << 12) | 0x23,

            //SB x1,0(x2)
            #[allow(clippy::identity_op)]
            1 => (1 << 20) | (2 << 15) | (0x0 << 12) | 0x23,

            x => panic!("Unrecognized memory size: {}", x),
        };
        self.write_instruction(bridge, inst)?;
        Ok(())
    }

    /// Actually read the value from a register
    ///
    /// Execute instructions on the CPU.  If reading a CSR, x1 will get clobbered.
    /// This clobbered value will be saved in the register cache.
    fn read_register(&self, bridge: &Bridge, reg: &RiscvRegister) -> Result<u32, RiscvCpuError> {
        match reg.register_type {
            RiscvRegisterType::General => {
                if reg.index == 32 {
                    self.write_instruction(bridge, 0x17) // AUIPC x0,0
                } else {
                    self.write_instruction(bridge, (reg.index << 15) | 0x13) // ADDI x0, x?, 0
                }
            }
            RiscvRegisterType::CSR => {
                // We clobber $x1 in this function, so read its previous value
                // (if we haven't already).
                // This will get restored when we resume.
                if self.get_cached_reg(&RiscvRegister::x1()).is_none() {
                    self.set_cached_reg(
                        &RiscvRegister::x1(),
                        self.read_register(bridge, &RiscvRegister::x1())?,
                    );
                }

                // Perform a CSRRW which does a Read/Write.  If rs1 is $x0, then the write
                // is ignored and side-effect free.  Set rd to $x1 to make the read
                // not side-effect free.
                #[allow(clippy::identity_op)]
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

    /// Write a value to a specified register
    ///
    /// Poke instructions into the CPU to update a specified register.  This might
    /// clobber register 1, and for CSRs might clobber register 2.  Clobbered values
    /// will be saved to the register cache.
    fn write_register(
        &self,
        bridge: &Bridge,
        reg: &RiscvRegister,
        value: u32,
    ) -> Result<(), RiscvCpuError> {
        debug!("Setting register {:?} -> {:08x}", reg, value);
        match reg.register_type {
            RiscvRegisterType::General => {
                // Handle PC separately
                if reg.index == 32 {
                    self.write_register(bridge, &RiscvRegister::x1(), value)?;
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
                if self.get_cached_reg(&RiscvRegister::x1()).is_none() {
                    self.set_cached_reg(
                        &RiscvRegister::x1(),
                        self.read_register(bridge, &RiscvRegister::x1())?,
                    );
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
                self.write_register(bridge, &RiscvRegister::x1(), value)?;
                #[allow(clippy::identity_op)]
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

    fn flush_cache(&self, bridge: &Bridge) -> Result<(), RiscvCpuError> {
        for opcode in &[4111, 19, 19, 19] {
            self.write_instruction(bridge, *opcode)?;
        }
        Ok(())
    }

    fn get_cached_reg(&self, reg: &RiscvRegister) -> Option<u32> {
        match self.cached_values.lock().unwrap().get(reg) {
            Some(x) => Some(*x),
            None => None,
        }
    }

    fn set_cached_reg(&self, reg: &RiscvRegister, value: u32) {
        self.cached_values
            .lock()
            .unwrap()
            .insert(reg.clone(), value);
    }

    fn write_instruction(&self, bridge: &Bridge, opcode: u32) -> Result<(), RiscvCpuError> {
        // debug!(
        //     "WRITE INSTRUCTION: 0x{:08x} -- 0x{:08x}",
        //     opcode,
        //     swab(opcode)
        // );
        bridge.poke(self.debug_offset + 4, opcode)?;
        for _ in 0..100 {
            if (self.read_status(bridge)? & VexRiscvFlags::PIP_BUSY) != VexRiscvFlags::PIP_BUSY {
                return Ok(());
            }
        }
        Err(RiscvCpuError::InstructionTimeout)
    }

    fn read_result(&self, bridge: &Bridge) -> Result<u32, RiscvCpuError> {
        Ok(bridge.peek(self.debug_offset + 4)?)
    }

    fn write_status(&self, bridge: &Bridge, value: VexRiscvFlags) -> Result<(), RiscvCpuError> {
        debug!("SETTING BRIDGE STATUS: {:08x}", value.bits);
        bridge.poke(self.debug_offset, value.bits)?;
        Ok(())
    }
}

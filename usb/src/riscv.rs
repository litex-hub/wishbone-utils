use super::bridge::{Bridge, BridgeError};

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

#[derive(Debug)]
pub enum RiscvCpuError {
    /// Someone tried to request an unrecognized feature file
    UnrecognizedFile(String /* requested filename */),
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

struct RiscvRegister {
    /// Which register group this belongs to
    register_type: RiscvRegisterType,

    /// Index within its namespace (e.g. `ustatus` is a CSR with index 0x000,
    /// even though GDB registers are offset by 65, so GDB calls `ustatus` register 65.)
    index: u32,

    /// Architecture name
    name: String,

    /// Whether this register is present on this device
    present: bool,
}

impl RiscvRegister {
    pub fn general(index: u32, name: &str) -> RiscvRegister {
        RiscvRegister {
            register_type: RiscvRegisterType::General,
            index,
            name: name.to_string(),
            present: true,
        }
    }

    pub fn csr(index: u32, name: &str, present: bool) -> RiscvRegister {
        RiscvRegister {
            register_type: RiscvRegisterType::CSR,
            index,
            name: name.to_string(),
            present,
        }
    }
}

pub struct RiscvCpu {
    /// A list of all available registers on this CPU
    registers: Vec<RiscvRegister>,

    /// An XML representation of the register mapping
    target_xml: String,

    /// The memory offset of the debug register
    debug_offset: u32,
}

impl RiscvCpu {
    pub fn new() -> Result<RiscvCpu, RiscvCpuError> {
        let registers = Self::make_registers();
        let target_xml = Self::make_target_xml(&registers);
        Ok(RiscvCpu {
            registers,
            target_xml,
            debug_offset: 0xf00f0000,
        })
    }

    fn make_registers() -> Vec<RiscvRegister> {
        let mut registers = vec![];

        // Add in general purpose registers x0 to x31
        for reg_num in 0..32 {
            registers.push(RiscvRegister::general(reg_num, &format!("x{}", reg_num)));
        }

        // Add the program counter
        registers.push(RiscvRegister::general(32, "pc"));

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
        registers.push(RiscvRegister::csr(0xc00, "cycle", true));
        registers.push(RiscvRegister::csr(0xc01, "time", false));
        registers.push(RiscvRegister::csr(0xc02, "instret", false));
        for hpmcounter_n in 3..32 {
            registers.push(RiscvRegister::csr(
                0xc00 + hpmcounter_n,
                &format!("hpmcounter{}", hpmcounter_n),
                false,
            ));
        }
        registers.push(RiscvRegister::csr(0xc80, "cycleh", true));
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
        registers.push(RiscvRegister::csr(0x100, "sstatus", true));
        registers.push(RiscvRegister::csr(0x102, "sedeleg", false));
        registers.push(RiscvRegister::csr(0x103, "sideleg", false));
        registers.push(RiscvRegister::csr(0x104, "sie", true));
        registers.push(RiscvRegister::csr(0x105, "stvec", true));
        registers.push(RiscvRegister::csr(0x106, "scounteren", true));

        // Supervisor Trap Handling
        registers.push(RiscvRegister::csr(0x140, "sscratch", true));
        registers.push(RiscvRegister::csr(0x141, "sepc", true));
        registers.push(RiscvRegister::csr(0x142, "scause", true));
        registers.push(RiscvRegister::csr(0x143, "stval", true));
        registers.push(RiscvRegister::csr(0x144, "sip", true));

        // Supervisor protection and translation
        registers.push(RiscvRegister::csr(0x180, "satp", true));

        // Machine information registers
        registers.push(RiscvRegister::csr(0xf11, "mvendorid", true));
        registers.push(RiscvRegister::csr(0xf12, "marchid", true));
        registers.push(RiscvRegister::csr(0xf13, "mimpid", true));
        registers.push(RiscvRegister::csr(0xf14, "mhartid", true));

        // Machine trap setup
        registers.push(RiscvRegister::csr(0x300, "mstatus", true));
        registers.push(RiscvRegister::csr(0x301, "misa", true));
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
                target_xml.push_str(
                    &format!("<reg name=\"{}\" bitsize=\"32\" regnum=\"{}\" save-restore=\"no\" type=\"int\" group=\"{}\"/>\n",
                        reg.name, reg.index, reg.register_type.group())
                );
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

    pub fn read_memory(&self, bridge: &Bridge, addr: u32, sz: u32) -> Result<u32, BridgeError> {
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
        self.read_result(bridge)
    }

    pub fn halt(&self, bridge: &Bridge) -> Result<(), BridgeError> {
        self.write_status(bridge, VexRiscvFlags::HALT_SET)
    }

    pub fn resume(&self, bridge: &Bridge) -> Result<(), BridgeError> {
        self.write_status(
            bridge,
            VexRiscvFlags::HALT_CLEAR | VexRiscvFlags::RESET_CLEAR,
        )
    }

    pub fn step(&self, bridge: &Bridge) -> Result<(), BridgeError> {
        self.write_status(bridge, VexRiscvFlags::HALT_CLEAR | VexRiscvFlags::STEP)
    }

    /* --- */
    fn write_register(&self, bridge: &Bridge, reg: u32, value: u32) -> Result<(), BridgeError> {
        assert!(reg <= 32);
        // Use LUI instruction if necessary
        if (value & 0xffff_f800) != 0 {
            let low = value & 0x0000_0fff;
            let high = if (low & 0x800) != 0 {
                (value & 0xffff_f000) + 0x1000
            } else {
                value & 0xffff_f000
            };

            // Also issue ADDI
            if low != 0 {
                // LUI regId, high
                self.write_instruction(bridge, 0x37 | (reg << 7) | high)?;

                // ADDI regId, regId, low
                self.write_instruction(bridge, 0x13 | (reg << 7) | (reg << 15) | (low << 20))
            } else {
                // LUI regId, high
                self.write_instruction(bridge, 0x37 | (reg << 7) | high)
            }
        } else {
            // ORI regId, x0, value
            self.write_instruction(bridge, 0x13 | (reg << 7) | (6 << 12) | (value << 20))
        }
    }
    /* --- */
    fn write_status(&self, bridge: &Bridge, value: VexRiscvFlags) -> Result<(), BridgeError> {
        bridge.poke(self.debug_offset, value.bits)
    }

    fn read_status(&self, bridge: &Bridge) -> Result<VexRiscvFlags, BridgeError> {
        match bridge.peek(self.debug_offset) {
            Err(e) => Err(e),
            Ok(bits) => Ok(VexRiscvFlags { bits }),
        }
    }

    fn write_instruction(&self, bridge: &Bridge, value: u32) -> Result<(), BridgeError> {
        bridge.poke(self.debug_offset + 4, value)
    }

    fn read_result(&self, bridge: &Bridge) -> Result<u32, BridgeError> {
        bridge.peek(self.debug_offset + 4)
    }
}

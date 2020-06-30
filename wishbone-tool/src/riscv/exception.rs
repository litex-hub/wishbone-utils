use std::fmt;

#[derive(PartialEq)]
pub enum RiscvException {
    /// When things are all 0
    NoException,

    /// 1 0
    UserSoftwareInterrupt(u32 /* mepc */),

    /// 1 1
    SupervisorSoftwareInterrupt(u32 /* mepc */),

    // [reserved]
    /// 1 3
    MachineSoftwareInterrupt(u32 /* mepc */),

    /// 1 4
    UserTimerInterrupt(u32 /* mepc */),

    /// 1 5
    SupervisorTimerInterrupt(u32 /* mepc */),

    // [reserved]
    /// 1 7
    MachineTimerInterrupt(u32 /* mepc */),

    /// 1 8
    UserExternalInterrupt(u32 /* mepc */),

    /// 1 9
    SupervisorExternalInterrupt(u32 /* mepc */),

    // [reserved]
    /// 1 11
    MachineExternalInterrupt(u32 /* mepc */),

    ReservedInterrupt(u32 /* unknown cause number */, u32 /* mepc */),

    /// 0 0
    InstructionAddressMisaligned(u32 /* mepc */, u32 /* target address */),

    /// 0 1
    InstructionAccessFault(u32 /* mepc */, u32 /* target address */),

    /// 0 2
    IllegalInstruction(u32 /* mepc */, u32 /* instruction value */),

    /// 0 3
    Breakpoint(u32 /* mepc */),

    /// 0 4
    LoadAddressMisaligned(u32 /* mepc */, u32 /* target address */),

    /// 0 5
    LoadAccessFault(u32 /* mepc */, u32 /* target address */),

    /// 0 6
    StoreAddressMisaligned(u32 /* mepc */, u32 /* target address */),

    /// 0 7
    StoreAccessFault(u32 /* mepc */, u32 /* target address */),

    /// 0 8
    CallFromUMode(u32 /* mepc */),

    /// 0 9
    CallFromSMode(u32 /* mepc */),

    // [reserved]
    /// 0 11
    CallFromMMode(u32 /* mepc */),

    /// 0 12
    InstructionPageFault(u32 /* mepc */, u32 /* target address */),

    /// 0 13
    LoadPageFault(u32 /* mepc */, u32 /* target address */),

    // [reserved]
    /// 0 15
    StorePageFault(u32 /* mepc */, u32 /* target address */),

    ReservedFault(
        u32, /* unknown cause number */
        u32, /* mepc */
        u32, /* mtval */
    ),
}

impl fmt::Display for RiscvException {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        use RiscvException::*;
        match *self {
            NoException => write!(f, "No trap"),
            UserSoftwareInterrupt(epc) => write!(f, "User swi from 0x{:08x}", epc),
            SupervisorSoftwareInterrupt(epc) => write!(f, "Supervisor swi from 0x{:08x}", epc),
            // --reserved--
            MachineSoftwareInterrupt(epc) => write!(f, "Machine swi at 0x{:08x}", epc),
            UserTimerInterrupt(epc) => write!(f, "User timer interrupt at 0x{:08x}", epc),
            SupervisorTimerInterrupt(epc) => {
                write!(f, "Supervisor timer interrupt at 0x{:08x}", epc)
            }
            // --reserved--
            MachineTimerInterrupt(epc) => write!(f, "Machine timer interrupt at 0x{:08x}", epc),
            UserExternalInterrupt(epc) => write!(f, "User external interrupt at 0x{:08x}", epc),
            SupervisorExternalInterrupt(epc) => {
                write!(f, "Machine external interrupt at 0x{:08x}", epc)
            }
            // --reserved--
            MachineExternalInterrupt(epc) => {
                write!(f, "Supervisor external interrupt at 0x{:08x}", epc)
            }
            ReservedInterrupt(code, epc) => {
                write!(f, "Reserved interrupt 0x{:08x} at 0x{:08x}", code, epc)
            }

            InstructionAddressMisaligned(epc, mtval) => write!(
                f,
                "Misaligned address instruction 0x{:08x} at 0x{:08x}",
                mtval, epc
            ),
            InstructionAccessFault(epc, mtval) => write!(
                f,
                "Instruction access fault to 0x{:08x} at 0x{:08x}",
                mtval, epc
            ),
            IllegalInstruction(epc, mtval) => {
                write!(f, "Illegal instruction 0x{:08x} at 0x{:08x}", mtval, epc)
            }
            Breakpoint(epc) => write!(f, "Breakpoint at 0x{:08x}", epc),
            LoadAddressMisaligned(epc, mtval) => write!(
                f,
                "Misaligned load address of 0x{:08x} at 0x{:08x}",
                mtval, epc
            ),
            LoadAccessFault(epc, mtval) => {
                write!(f, "Load access fault from 0x{:08x} at 0x{:08x}", mtval, epc)
            }
            StoreAddressMisaligned(epc, mtval) => write!(
                f,
                "Misaligned store address of 0x{:08x} at 0x{:08x}",
                mtval, epc
            ),
            StoreAccessFault(epc, mtval) => {
                write!(f, "Store access fault to 0x{:08x} at 0x{:08x}", mtval, epc)
            }
            CallFromUMode(epc) => write!(f, "Call from User mode at 0x{:08x}", epc),
            CallFromSMode(epc) => write!(f, "Call from Supervisor mode at 0x{:08x}", epc),
            // --reserved--
            CallFromMMode(epc) => write!(f, "Call from Machine mode at 0x{:08x}", epc),
            InstructionPageFault(epc, mtval) => write!(
                f,
                "Instruction page fault of 0x{:08x} at 0x{:08x}",
                mtval, epc
            ),
            LoadPageFault(epc, mtval) => {
                write!(f, "Load page fault of 0x{:08x} at 0x{:08x}", mtval, epc)
            }
            // --reserved--
            StorePageFault(epc, mtval) => {
                write!(f, "Load page fault of 0x{:08x} at 0x{:08x}", mtval, epc)
            }
            ReservedFault(code, epc, mtval) => write!(
                f,
                "Reserved interrupt 0x{:08x} with cause 0x{:08x} at 0x{:08x}",
                code, mtval, epc
            ),
        }
    }
}

impl RiscvException {
    pub fn from_regs(mcause: u32, mepc: u32, mtval: u32) -> RiscvException {
        use RiscvException::*;

        if mepc == 0 && mtval == 0 {
            return NoException;
        }

        match mcause {
            0x8000_0000 => UserSoftwareInterrupt(mepc),
            0x8000_0001 => SupervisorSoftwareInterrupt(mepc),
            // --reserved--
            0x8000_0003 => MachineSoftwareInterrupt(mepc),
            0x8000_0004 => UserTimerInterrupt(mepc),
            0x8000_0005 => SupervisorTimerInterrupt(mepc),
            // --reserved--
            0x8000_0007 => MachineTimerInterrupt(mepc),
            0x8000_0008 => UserExternalInterrupt(mepc),
            0x8000_0009 => SupervisorExternalInterrupt(mepc),
            // --reserved--
            0x8000_000b => MachineExternalInterrupt(mepc),
            x @ 0x8000_0002 | x @ 0x8000_0006 | x @ 0x8000_000a | x @ 0x8000_000c..=0xffff_ffff => {
                ReservedInterrupt(x & 0x7fff_ffff, mepc)
            }

            0 => InstructionAddressMisaligned(mepc, mtval),
            1 => InstructionAccessFault(mepc, mtval),
            2 => IllegalInstruction(mepc, mtval),
            3 => Breakpoint(mepc),
            4 => LoadAddressMisaligned(mepc, mtval),
            5 => LoadAccessFault(mepc, mtval),
            6 => StoreAddressMisaligned(mepc, mtval),
            7 => StoreAccessFault(mepc, mtval),
            8 => CallFromUMode(mepc),
            9 => CallFromSMode(mepc),
            // --reserved--
            11 => CallFromMMode(mepc),
            12 => InstructionPageFault(mepc, mtval),
            13 => LoadPageFault(mepc, mtval),
            // --reserved--
            15 => StorePageFault(mepc, mtval),
            x @ 10 | x @ 14 | x @ 16..=0x7fff_ffff => ReservedFault(x, mepc, mtval),
        }
    }
}

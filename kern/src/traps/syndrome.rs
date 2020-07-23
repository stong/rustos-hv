use aarch64::ESR_EL1;

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum Fault {
    AddressSize,
    Translation,
    AccessFlag,
    Permission,
    Alignment,
    TlbConflict,
    Other(u8),
}

impl From<u32> for Fault {
    fn from(val: u32) -> Fault {
        use self::Fault::*;
        match ((val >> 2) & 0b1111) as u8{
            0b0000 => AddressSize,
            0b0001 => Translation,
            0b0010 => AccessFlag,
            0b0011 => Permission,
            0b0100 | 0b1000 => Alignment, // IDK this seems to work
            0b1100 => TlbConflict,
            x => Other(x)
        }
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum Syndrome {
    Unknown,
    WfiWfe,
    SimdFp,
    IllegalExecutionState,
    Svc(u16),
    Hvc(u16),
    Smc(u16),
    MsrMrsSystem,
    InstructionAbort { kind: Fault, level: u8 },
    PCAlignmentFault,
    DataAbort { kind: Fault, level: u8 },
    SpAlignmentFault,
    TrappedFpu,
    SError,
    Breakpoint,
    Step,
    Watchpoint,
    Brk(u16),
    Other(u32),
}

/// Converts a raw syndrome value (ESR) into a `Syndrome` (ref: D1.10.4).
impl From<u32> for Syndrome {
    fn from(esr: u32) -> Syndrome {
        use self::Syndrome::*;
        let ec = esr >> 26;
        match ec {
            0b000000 => Unknown,
            0b000001 => WfiWfe,
            0b000111 => SimdFp,
            0b001110 => IllegalExecutionState,
            0b010001 | 0b010101 => Svc(esr as u16),
            0b010010 | 0b010110 => Hvc(esr as u16),
            0b010011 | 0b010111 => Smc(esr as u16),
            0b011000 => MsrMrsSystem,
            0b100000 | 0b100001 => InstructionAbort{kind: Fault::from(esr & 0b111111), level: (esr & 0b11) as u8},
            0b100010 => PCAlignmentFault,
            0b100100 | 0b100101 => DataAbort{kind: Fault::from(esr & 0b111111), level: (esr & 0b11) as u8},
            0b100110 => SpAlignmentFault,
            0b101100 | 0b101000 => TrappedFpu,
            0b101111 => SError,
            0b110000 | 0b110001 | 0b111000 => Breakpoint,
            0b110010 | 0b110011 => Step,
            0b110100 | 0b110101 => Watchpoint,
            0b111100 => Brk(esr as u16),
            _ => Other(esr)
        }
    }
}

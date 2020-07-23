#[macro_use]
use aarch64::*;

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

defbit!(DataAbortSyndrome, [
    ISV    [24-24], // valid
    SAS    [23-22], // 0,1,2,3 = byte, halfword, word, doubleword
    SSE    [21-21], // 1 = sign extend
    SRT    [20-16], // destination register number
    SF     [15-15], // register width (1=Instruction loads/stores a 64-bit wide register, 0=32)
    AR     [14-14], // 1 = acquire-release semantics
    SET    [12-11], // ???? Synchronous Error Type
    FnV    [10-10], // FAR not valid
    EA     [9-9],   // external abort type (basically zero)
    CM     [8-8],   // fault happened because user was messing with caches like with DC
    S1PTW  [7-7],   // for stage 2 fault, whether the fault was a stage 2 fault on an access made for a stage 1 translation table walk
    WnR    [6-6],   // 0 = read, 1 = write
    DFSC   [5-0],   // kind and level
]);

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum Syndrome {
    Unknown(u32),
    WfiWfe,
    SimdFp,
    IllegalExecutionState,
    Svc(u16),
    Hvc(u16),
    Smc(u16),
    MsrMrsSystem,
    InstructionAbort { kind: Fault, level: u8 },
    PCAlignmentFault,
    DataAbort { kind: Fault, level: u8, iss: DataAbortSyndrome },
    SpAlignmentFault,
    TrappedFpu,
    SError,
    Breakpoint,
    Step,
    Watchpoint,
    Brk(u16),
    Other(u32),
}

impl Syndrome {
    pub fn get_abort_info(&self) -> Option<(Fault, u8)> {
        match self {
            Syndrome::InstructionAbort{kind, level} => Some((*kind, *level)),
            Syndrome::DataAbort{kind, level, iss} => Some((*kind, *level)),
            _ => None
        }
    }
}

/// Converts a raw syndrome value (ESR) into a `Syndrome` (ref: D1.10.4).
impl From<u32> for Syndrome {
    fn from(esr: u32) -> Syndrome {
        use self::Syndrome::*;
        let ec = esr >> 26;
        match ec {
            0b000000 => Unknown(esr),
            0b000001 => WfiWfe,
            0b000111 => SimdFp,
            0b001110 => IllegalExecutionState,
            0b010001 | 0b010101 => Svc(esr as u16),
            0b010010 | 0b010110 => Hvc(esr as u16),
            0b010011 | 0b010111 => Smc(esr as u16),
            0b011000 => MsrMrsSystem,
            0b100000 | 0b100001 => InstructionAbort{kind: Fault::from(esr & 0b111111), level: (esr & 0b11) as u8},
            0b100010 => PCAlignmentFault,
            0b100100 | 0b100101 => DataAbort{kind: Fault::from(esr & 0b111111), level: (esr & 0b11) as u8, iss: DataAbortSyndrome::new(esr as u64 & 0x1FFFFFF)},
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

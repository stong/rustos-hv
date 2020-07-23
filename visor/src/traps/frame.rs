use core::fmt;

#[repr(C, align(16))]
#[derive(Default, Copy, Clone, Debug)]
#[allow(non_snake_case)]
pub struct TrapFrame {
    pub VTTBR: u64,
    pub ELR: u64,
    pub TTBR0_EL1: u64,
    pub TTBR1_EL1: u64,
    pub SP_EL0: u64,
    pub SP_EL1: u64,
    pub SCTLR_EL1: u64,
    pub VBAR_EL1: u64,
    pub TPIDR_EL0: u64,
    pub TPIDR_EL1: u64,
    pub SPSR_EL1: u64,
    _pad1: u64,
    pub qn: [u128; 32],
    pub xn: [u64; 32] // lr = x30, xzr = x31
}

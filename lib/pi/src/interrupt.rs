use crate::common::IO_BASE;

use volatile::prelude::*;
use volatile::{Volatile, ReadVolatile};

const INT_BASE: usize = IO_BASE + 0xB000 + 0x200;

#[derive(Copy, Clone, PartialEq)]
pub enum Interrupt {
    Timer1 = 1,
    Timer3 = 3,
    Usb = 9,
    Gpio0 = 49,
    Gpio1 = 50,
    Gpio2 = 51,
    Gpio3 = 52,
    Uart = 57,
}

impl Interrupt {
    pub const MAX: usize = 8;

    pub fn iter() -> core::slice::Iter<'static, Interrupt> {
        use Interrupt::*;
        [Timer1, Timer3, Usb, Gpio0, Gpio1, Gpio2, Gpio3, Uart].into_iter()
    }

    pub fn to_index(i: Interrupt) -> usize {
        use Interrupt::*;
        match i {
            Timer1 => 0,
            Timer3 => 1,
            Usb => 2,
            Gpio0 => 3,
            Gpio1 => 4,
            Gpio2 => 5,
            Gpio3 => 6,
            Uart => 7,
        }
    }

    pub fn from_index(i: usize) -> Interrupt {
        use Interrupt::*;
        match i {
            0 => Timer1,
            1 => Timer3,
            2 => Usb,
            3 => Gpio0,
            4 => Gpio1,
            5 => Gpio2,
            6 => Gpio3,
            7 => Uart,
            _ => panic!("Unknown interrupt: {}", i),
        }
    }
}


impl From<usize> for Interrupt {
    fn from(irq: usize) -> Interrupt {
        use Interrupt::*;
        match irq {
            1 => Timer1,
            3 => Timer3,
            9 => Usb,
            49 => Gpio0,
            50 => Gpio1,
            51 => Gpio2,
            52 => Gpio3,
            57 => Uart,
            _ => panic!("Unkonwn irq: {}", irq),
        }
    }
}

#[repr(C)]
#[allow(non_snake_case)]
struct Registers {
    IRQ_basic_pending: ReadVolatile<u32>,
    IRQ_pending_1: ReadVolatile<u32>,
    IRQ_pending_2: ReadVolatile<u32>,
    FIQ_control: Volatile<u32>,
    Enable_IRQ_1: Volatile<u32>,
    Enable_IRQ_2: Volatile<u32>,
    Enable_Basic_IRQ: Volatile<u32>,
    Disable_IRQ_1: Volatile<u32>,
    Disable_IRQ_2: Volatile<u32>,
    Disable_Basic_IRQ: Volatile<u32>,
}

/// An interrupt controller. Used to enable and disable interrupts as well as to
/// check if an interrupt is pending.
pub struct Controller {
    registers: &'static mut Registers
}

impl Controller {
    /// Returns a new handle to the interrupt controller.
    pub fn new() -> Controller {
        Controller {
            registers: unsafe { &mut *(INT_BASE as *mut Registers) },
        }
    }

    /// Enables the interrupt `int`.
    pub fn enable(&mut self, int: Interrupt) {
        let irq_idx = int as u32;
        if irq_idx < 32 {
            self.registers.Enable_IRQ_1.or_mask(1 << irq_idx);
        } else {
            self.registers.Enable_IRQ_2.or_mask(1 << (irq_idx - 32));
        }
    }

    /// Disables the interrupt `int`.
    pub fn disable(&mut self, int: Interrupt) {
        let irq_idx = int as u32;
        if irq_idx < 32 {
            self.registers.Enable_IRQ_1.or_mask(1 << irq_idx);
        } else {
            self.registers.Enable_IRQ_2.or_mask(1 << (irq_idx - 32));
        }
    }

    /// Returns `true` if `int` is pending. Otherwise, returns `false`.
    pub fn is_pending(&self, int: Interrupt) -> bool {
        let irq_idx = int as u32;
        if irq_idx < 32 {
            self.registers.Enable_IRQ_1.read() & (1 << irq_idx) != 0
        } else {
            self.registers.Enable_IRQ_2.read() & (1 << (irq_idx - 32)) != 0
        }
    }
}

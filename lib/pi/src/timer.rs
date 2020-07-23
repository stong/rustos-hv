use crate::common::IO_BASE;
use core::time::Duration;

use volatile::prelude::*;
use volatile::{ReadVolatile, Volatile};

/// The base address for the ARM system timer registers.
const TIMER_REG_BASE: usize = IO_BASE + 0x3000;

#[repr(C)]
#[allow(non_snake_case)]
struct Registers {
    CS: Volatile<u32>,
    CLO: ReadVolatile<u32>,
    CHI: ReadVolatile<u32>,
    COMPARE: [Volatile<u32>; 4],
}

/// The Raspberry Pi ARM system timer.
pub struct Timer {
    registers: &'static mut Registers,
}

impl Timer {
    /// Returns a new instance of `Timer`.
    pub fn new() -> Timer {
        Timer {
            registers: unsafe { &mut *(TIMER_REG_BASE as *mut Registers) },
        }
    }

    /// Reads the system timer's counter and returns Duration.
    /// `CLO` and `CHI` together can represent the number of elapsed microseconds.
    pub fn read(&self) -> Duration {
        Duration::from_micros(((self.registers.CHI.read() as u64) << 32) | (self.registers.CLO.read() as u64))
    }

    /// Sets up a match in timer 1 to occur `t` duration from now. If
    /// interrupts for timer 1 are enabled and IRQs are unmasked, then a timer
    /// interrupt will be issued in `t` duration.
    pub fn tick_in(&mut self, t: Duration) {
        // clear match detect status
        self.registers.CS.write(1 << 1);
        // set next time
        self.registers.COMPARE[1].write((self.read() + t).as_micros() as u32)
    }
}

/// Returns current time.
pub fn current_time() -> Duration {
    let timer = Timer::new();
    timer.read()
}

/// Spins until `t` duration have passed.
pub fn spin_sleep(t: Duration) {
    let cur_time = current_time();
    while current_time() - cur_time < t {
        unsafe { asm!("nop" :::: "volatile"); }
    }
}

/// Sets up a match in timer 1 to occur `t` duration from now. If
/// interrupts for timer 1 are enabled and IRQs are unmasked, then a timer
/// interrupt will be issued in `t` duration.
pub fn tick_in(t: Duration) {
    let mut timer = Timer::new();
    timer.tick_in(t)
}

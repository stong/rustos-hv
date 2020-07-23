use volatile::prelude::*;
use crate::common::IO_BASE;
use volatile::{WriteVolatile, Volatile};

// https://github.com/bztsrc/raspi3-tutorial/blob/master/08_power/power.c#L75
const POWER_REG_BASE: usize = IO_BASE + 0x0010001c;
const PM_WDOG_MAGIC: u32 = 0x5a000000;
const PM_RSTC_FULLRST: u32 = 0x00000020;

#[repr(C)]
#[allow(non_snake_case)]
struct Registers {
    PM_RSTC: WriteVolatile<u32>,
    PM_RSTS: Volatile<u32>,
    PM_WDOG: WriteVolatile<u32>
}

pub struct PowerManager {
    registers: &'static mut Registers,
}

impl PowerManager {
    /// Returns a new instance of `PowerManager`.
    pub fn new() -> PowerManager {
        PowerManager {
            registers: unsafe { &mut *(POWER_REG_BASE as *mut Registers) },
        }
    }

    // Hard resets the cpu. (bare) metal af.
    pub unsafe fn reset(&mut self) -> ! {
        // trigger a restart by instructing the CPU to boot from partition 0
        let r: u32 = self.registers.PM_RSTS.read() & !0xfffffaaa;
        self.registers.PM_RSTS.write(PM_WDOG_MAGIC | r); // boot from partition 0
        self.registers.PM_WDOG.write(PM_WDOG_MAGIC | 10);
        self.registers.PM_RSTC.write(PM_WDOG_MAGIC | PM_RSTC_FULLRST);
        unreachable!("reset")
    }
}

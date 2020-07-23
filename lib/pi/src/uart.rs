use core::fmt;
use core::time::Duration;

use shim::const_assert_size;
use shim::io;

use volatile::prelude::*;
use volatile::{ReadVolatile, Reserved, Volatile};

use crate::common::IO_BASE;
use crate::gpio::{Function, Gpio};
use crate::timer;

/// The base address for the `MU` registers.
const MU_REG_BASE: usize = IO_BASE + 0x215040;

/// The `AUXENB` register from page 9 of the BCM2837 documentation.
const AUX_ENABLES: *mut Volatile<u8> = (IO_BASE + 0x215004) as *mut Volatile<u8>;

/// Enum representing bit fields of the `AUX_MU_LSR_REG` register.
#[repr(u8)]
enum LsrStatus {
    DataReady = 1,
    TxAvailable = 1 << 5,
}

#[repr(C)]
#[allow(non_snake_case)]
struct Registers {
    IO: Volatile<u8>,
    __r0: [Reserved<u8>; 3],
    IER: Volatile<u8>,
    __r1: [Reserved<u8>; 3],
    IIR: Volatile<u8>,
    __r2: [Reserved<u8>; 3],
    LCR: Volatile<u8>,
    __r3: [Reserved<u8>; 3],
    MCR: Volatile<u8>,
    __r4: [Reserved<u8>; 3],
    LSR: ReadVolatile<u8>,
    __r5: [Reserved<u8>; 3],
    MSR: ReadVolatile<u8>,
    __r6: [Reserved<u8>; 3],
    SCRATCH: Volatile<u8>,
    __r7: [Reserved<u8>; 3],
    CNTL: Volatile<u8>,
    __r8: [Reserved<u8>; 3],
    STAT: ReadVolatile<u32>,
    BAUD: Volatile<u16>,
}

const_assert_size!(Registers, 0x7E21506C - 0x7E215040);

/// The Raspberry Pi's "mini UART".
pub struct MiniUart {
    registers: &'static mut Registers,
    timeout: Option<Duration>,
}

impl MiniUart {
    /// Initializes the mini UART by enabling it as an auxiliary peripheral,
    /// setting the data size to 8 bits, setting the BAUD rate to ~115200 (baud
    /// divider of 270), setting GPIO pins 14 and 15 to alternative function 5
    /// (TXD1/RDXD1), and finally enabling the UART transmitter and receiver.
    ///
    /// By default, reads will never time out. To set a read timeout, use
    /// `set_read_timeout()`.
    pub fn new() -> MiniUart {
        let registers = unsafe {
            // Enable the mini UART as an auxiliary device.
            (*AUX_ENABLES).or_mask(1);
            &mut *(MU_REG_BASE as *mut Registers)
        };

        registers.LCR.write(0b11); // 8-bit mode, no DLAB
        registers.BAUD.write(270); // baudrate 115200
        registers.CNTL.write(0b11); // tx/rx enable, no flow control

        Gpio::new(14).into_alt(Function::Alt5);
        Gpio::new(15).into_alt(Function::Alt5);

        MiniUart {
            registers: registers,
            timeout: None
        }
    }

    /// Set the read timeout to `t` duration.
    pub fn set_read_timeout(&mut self, t: Duration) {
        self.timeout = Some(t);
    }

    /// Returns `true` if there is space available in the tx fifo
    fn tx_ready(&mut self) -> bool {
        self.registers.STAT.read() & (1<<1) != 0
    }

    /// Returns `true` if the tx fifo is empty
    fn tx_empty(&mut self) -> bool {
        self.registers.STAT.read() & (1<<8) != 0
    }

    /// Blocks until timeout expires if one is configured, or indefinitely until
    /// the tx fifo is empty.
    ///
    /// Returns `Ok(())` if fifo is flushed. Returns `Err(())` if the
    /// timeout expired while waiting. If this method returns `Ok(())`,
    /// a subsequent call to `write_byte` is guaranteed to return immediately.
    pub fn flush_tx_fifo(&mut self) -> Result<(), ()> {
        let start_time = timer::current_time();
        while !self.tx_empty() {
            if self.timeout.is_some() && timer::current_time() > start_time + self.timeout.unwrap() {
                return Err(());
            }
        }
        Ok(())
    }

    /// Write the byte `byte`. This method blocks until there is space available
    /// in the output FIFO.
    pub fn write_byte(&mut self, byte: u8) {
        // This is TOCTOU!!!
        while !self.tx_ready() { unsafe { asm!("nop" :::: "volatile"); } }
        self.registers.IO.write(byte);
    }

    /// Returns `true` if there is at least one byte ready to be read. If this
    /// method returns `true`, a subsequent call to `read_byte` is guaranteed to
    /// return immediately. This method does not block.
    pub fn has_byte(&self) -> bool {
        self.registers.STAT.read() & (1<<0) != 0
    }

    /// Blocks until there is a byte ready to read. If a read timeout is set,
    /// this method blocks for at most that amount of time. Otherwise, this
    /// method blocks indefinitely until there is a byte to read.
    ///
    /// Returns `Ok(())` if a byte is ready to read. Returns `Err(())` if the
    /// timeout expired while waiting for a byte to be ready. If this method
    /// returns `Ok(())`, a subsequent call to `read_byte` is guaranteed to
    /// return immediately.
    pub fn wait_for_byte(&self) -> Result<(), ()> {
        let start_time = timer::current_time();
        while !self.has_byte() {
            if self.timeout.is_some() && timer::current_time() > start_time + self.timeout.unwrap() {
                return Err(());
            }
        }
        Ok(())
    }

    /// Reads a byte. Blocks indefinitely until a byte is ready to be read.
    pub fn read_byte(&mut self) -> u8 {
        // This is TOCTOU!!!
        while !self.has_byte() { unsafe { asm!("nop" :::: "volatile"); } }
        self.registers.IO.read()
    }
}

// Implement `fmt::Write` for `MiniUart`. A b'\r' byte should be written
// before writing any b'\n' byte.
impl fmt::Write for MiniUart {
    fn write_str(&mut self, s: &str) -> Result<(), fmt::Error> {
        for c in s.chars() {
            if c == '\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(c as u8);
        }
        Ok(())
    }
}

mod uart_io {
    use super::io;
    use super::MiniUart;
    use shim::ioerr;
    use volatile::prelude::*;

    // The `io::Read::read()` implementation must respect the read timeout by
    // waiting at most that time for the _first byte_. It should not wait for
    // any additional bytes but _should_ read as many bytes as possible. If the
    // read times out, an error of kind `TimedOut` should be returned.
    impl io::Read for MiniUart {
        fn read(&mut self, buf: &mut[u8]) -> io::Result<usize> {
            if !self.wait_for_byte().is_ok() {
                return ioerr!(TimedOut, "read timed out");
            }
            for i in 0..buf.len() {
                buf[i] = self.read_byte();
                if !self.has_byte() {
                    return Ok(i + 1)
                }
            }
            return Ok(buf.len())
        }
    }

    // The `io::Write::write()` method must write all of the requested bytes
    // before returning.
    impl io::Write for MiniUart {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            for &byte in buf {
                self.write_byte(byte);
                if byte == b'\n' {
                    self.write_byte(b'\r')
                }
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            return match self.flush_tx_fifo() {
                Ok(_) => Ok(()),
                Err(_) => ioerr!(TimedOut, "flush timed out")
            }
        }
    }
}

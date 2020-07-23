mod frame;
mod syndrome;
mod syscall;

pub mod irq;
use crate::IRQ;
use crate::SCHEDULER;
pub use self::frame::TrapFrame;
use self::syscall::{sys_sleep};
use crate::param;
use crate::vm::{PhysicalAddr, VirtualAddr};
use crate::vm;
use crate::util;
use crate::mutex::ReentrantLock;

use aarch64::*;
use pi::interrupt::{Controller, Interrupt};

use self::syndrome::*;
use self::syscall::handle_syscall;

#[repr(u16)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum Kind {
    Synchronous = 0,
    Irq = 1,
    Fiq = 2,
    SError = 3,
}

#[repr(u16)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum Source {
    CurrentSpEl0 = 0,
    CurrentSpElx = 1,
    LowerAArch64 = 2,
    LowerAArch32 = 3,
}

#[repr(C)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct Info {
    source: Source,
    kind: Kind,
}

use crate::console::{kprintln};
use crate::shell::Shell;

fn handle_mmio(fault_addr: usize, iss: DataAbortSyndrome, tf: &mut TrapFrame) {
    assert!(fault_addr >= param::IO_BASE && fault_addr < param::IO_BASE_END);
    let sext = iss.get_value(DataAbortSyndrome::SSE) == 1;
    let regno = iss.get_value(DataAbortSyndrome::SRT) as usize;
    let write = iss.get_value(DataAbortSyndrome::WnR) == 1;
    let reg64 = iss.get_value(DataAbortSyndrome::SF) == 1;
    let access_size = iss.get_value(DataAbortSyndrome::SAS);
    // kprintln!("Emulating {} {:x}({}), with reg {}{}, sext={}", if write { "write to" } else { "read from" }, fault_addr, 8 << access_size, if reg64 { "x" } else { "w" }, regno, sext);
    if write {
        let mut data: u64 = tf.xn[regno];
        if !reg64 { // 32-bit register
            data &= 0xFFFFFFFF;
        }
        unsafe { match access_size {
            // sext dont apply for stores
            0 => *(fault_addr as *mut u8)  = data as u8,
            1 => *(fault_addr as *mut u16) = data as u16,
            2 => *(fault_addr as *mut u32) = data as u32,
            3 => *(fault_addr as *mut u64) = data as u64,
            _ => unreachable!()
        }};
    } else {
        let data: u64 = unsafe { match access_size {
            0 => (if sext { *(fault_addr as *mut i8)  as u64 } else { *(fault_addr as *mut u8)  as u64 }),
            1 => (if sext { *(fault_addr as *mut i16) as u64 } else { *(fault_addr as *mut u16) as u64 }),
            2 => (if sext { *(fault_addr as *mut i32) as u64 } else { *(fault_addr as *mut u32) as u64 }),
            3 => (if sext { *(fault_addr as *mut i64) as u64 } else { *(fault_addr as *mut u64) as u64 }),
            _ => unreachable!()
        }};
        tf.xn[regno] = if reg64 {
            data
        } else {
            (tf.xn[regno] & (0xFFFFFFFF00000000)) | (data & 0x00000000FFFFFFFF)
        };
    }
    tf.ELR += 4; // skip over emulated instruction
}

// // kern_base..max_vm
fn handle_lower_el_synchronous(info: Info, syndrome: Syndrome, far: u64, hpfar: u64, tf: &mut TrapFrame) {
    if let Some((kind, info)) = syndrome.get_abort_info() {
        if kind == Fault::AccessFlag || kind == Fault::Translation {
            let translation_fault_addr = ((hpfar >> 4) << 12) as usize;
            let fault_addr = far as usize;
            let fault_page = VirtualAddr::from(util::align_down(translation_fault_addr, param::PAGE_SIZE));
            if translation_fault_addr < param::GUEST_MAX_VM_SIZE {
                // lazy paging
                let vmid = VTTBR_EL2::get_masked(tf.VTTBR, VTTBR_EL2::VMID);
                let mut process = SCHEDULER.get_by_vmid(vmid as u8);
                let vmap = &mut process.vmap;
                if !vmap.get_entry(fault_page).is_valid() {
                    vmap.alloc(fault_page, vm::PagePerm::RWX);
                    return;
                }
            }
        }
    }

    match syndrome {
        Syndrome::DataAbort{kind, level, iss} => {
            if kind == Fault::Translation {
                if iss.get_value(DataAbortSyndrome::ISV) == 0 {
                    panic!("DataAbort ISS not vaid?");
                }
                if iss.get_value(DataAbortSyndrome::CM) == 0 {
                    let fault_addr = if iss.get_value(DataAbortSyndrome::FnV) != 0 {
                        ((hpfar >> 4) << 12) // FAR not valid
                    } else {
                        far
                    } as usize;
                    if fault_addr >= param::IO_BASE && fault_addr < param::IO_BASE_END {
                        handle_mmio(fault_addr, iss, tf);
                        return;
                    }
                } else {
                    kprintln!("Cache management abort?");
                }
            }
        },
        _ => {},
    }
    kprintln!("Received system exception at {:x}", tf.ELR);
    kprintln!("Exception info: {:?}", info);
    kprintln!("Context: {:?}", tf);
    kprintln!("VMID: {:?}", VTTBR_EL2::get_masked(tf.VTTBR, VTTBR_EL2::VMID));
    kprintln!("Syndrome: {:?}", syndrome);
    kprintln!("Fault address EL2: {:x}", far);
    kprintln!("Translation fault address: {:x}", (hpfar >> 4) << 12);
    Shell::new("! ").do_forever();
}

static DOUBLE_FAULT_LOCK: ReentrantLock = ReentrantLock::new();

/// This function is called when an exception occurs. The `info` parameter
/// specifies the source and kind of exception that has occurred. The `esr` is
/// the value of the exception syndrome register. Finally, `tf` is a pointer to
/// the trap frame for the exception.
#[no_mangle]
pub extern "C" fn handle_exception(info: Info, esr: u32, far: u64, hpfar: u64, tf: &mut TrapFrame) {
    let x = DOUBLE_FAULT_LOCK.enter();
    if info.source == Source::LowerAArch64 {
        if Kind::Synchronous == info.kind {
            let syndrome = Syndrome::from(esr);
            handle_lower_el_synchronous(info, syndrome, far, hpfar, tf);
            return
        } else if info.kind == Kind::Irq {
            let controller = Controller::new();
            for &interrupt in Interrupt::iter().filter(|&&i| controller.is_pending(i)) {
                // kprintln!("Interrupt {} is pending", interrupt as usize);
                IRQ.invoke(interrupt, tf);
            }
            return
        }
    } else {
        kprintln!("We messed up big time");
    }
    kprintln!("Received system exception at {:x}", tf.ELR);
    kprintln!("Exception info: {:?}", info);
    kprintln!("Source: {:x}", esr);
    kprintln!("Context: {:?}", tf);
    kprintln!("VMID: {:?}", VTTBR_EL2::get_masked(tf.VTTBR, VTTBR_EL2::VMID));
    Shell::new("! ").do_forever();
}

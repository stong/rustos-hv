#![allow(unused_variables, dead_code, unused_imports)]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]
#![feature(const_fn)]
#![feature(decl_macro)]
#![feature(asm)]
#![feature(global_asm)]
#![feature(optin_builtin_traits)]
#![feature(ptr_internals)]
#![feature(raw_vec_internals)]
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

#[cfg(not(test))]
mod init;

extern crate alloc;

pub mod allocator;
pub mod console;
pub mod fs;
pub mod mutex;
pub mod shell;
pub mod param;
pub mod process;
pub mod traps;
pub mod vm;
pub mod util;

use console::{kprintln};

use core::time::Duration;
use pi::timer;
use fs::FileSystem;
use shell::Shell;
use process::GlobalScheduler;
use traps::irq::Irq;
use vm::VMManager;

use allocator::Allocator;
use fs::sd::Sd;

#[cfg_attr(not(test), global_allocator)]
pub static ALLOCATOR: Allocator = Allocator::uninitialized();
pub static FILESYSTEM: FileSystem = FileSystem::uninitialized();
pub static SCHEDULER: GlobalScheduler = GlobalScheduler::uninitialized();
pub static VMM: VMManager = VMManager::uninitialized();
pub static IRQ: Irq = Irq::uninitialized();

use shim::io;
use shim::path::Path;
use kernel_api::{OsError, OsResult};

// THREE STEP PLAN TO VIRTUALIZATION
// 1. DO THE MEMORY. IPA MEMES
// enable HCR_EL0 to do the VTTBR0_EL2

// setup VTCR_EL2
// setup VTTBR_EL2
// setup TTBR0_EL2

// 2. DO THE EXCEPTIONS. (spsr_el2) 
// fix the exceptions
// lie about IRQs using HCR_EL0:IMO so we get the IRQs yee haw

// 3. DO THE DEVICE.
// virtualize all the mmio memes

fn kmain() -> ! {
    timer::spin_sleep(Duration::from_millis(1000));

    kprintln!("hypervisor: we are in EL{}", unsafe { aarch64::current_el() } );
    
    unsafe {
        ALLOCATOR.initialize();
        FILESYSTEM.initialize(Sd::new().unwrap());
        IRQ.initialize();
        VMM.initialize();
        SCHEDULER.initialize();
    }

    kprintln!("Welcome to cs3210!!");
    
    SCHEDULER.start()
    
    // Shell::new("# ").do_forever();
}

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

use console::{kprintln};

use core::time::Duration;
use pi::timer;
use fs::FileSystem;
use shell::Shell;
use process::GlobalScheduler;
use traps::irq::Irq;
use vm::VMManager;
use aarch64::current_el;

use allocator::Allocator;
use fs::sd::Sd;

#[cfg_attr(not(test), global_allocator)]
pub static ALLOCATOR: Allocator = Allocator::uninitialized();
pub static FILESYSTEM: FileSystem = FileSystem::uninitialized();
pub static SCHEDULER: GlobalScheduler = GlobalScheduler::uninitialized();
pub static VMM: VMManager = VMManager::uninitialized();
pub static IRQ: Irq = Irq::uninitialized();

fn kmain() -> ! {
    // timer::spin_sleep(Duration::from_millis(1000));
    
    kprintln!("kern: we are in EL{}", unsafe { current_el() } );
    for atag in pi::atags::Atags::get() {
        kprintln!("{:?}", atag);
    }
    kprintln!();

    unsafe {
        ALLOCATOR.initialize();
        FILESYSTEM.initialize(Sd::new().unwrap());
    }
    
    kprintln!("Welcome to cs3210!");

    unsafe {
        // asm!("hvc 1" :::: "volatile");
    }

    loop {
        Shell::new("$ ").do_forever();
    }
}

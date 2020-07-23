use shim::{const_assert_eq, const_assert_size};

// we only support 64-bit
const_assert_size!(usize, 64 / 8);

use core::time::Duration;
pub use pi::common::*;

pub const PAGE_ALIGN: usize = 16;
pub const PAGE_SIZE: usize = 64 * 1024;
pub const PAGE_MASK: usize = !(PAGE_SIZE - 1);

pub const GUEST_MASK_BITS: usize = 34;
pub const VISOR_MASK_BITS: usize = 32;

pub const KERN_START_ADDR: u64 = 0x80000u64;
pub const GUEST_MAX_VM_SIZE: usize = 0x1000_0000; // 256MiB
pub const KERN_STACK_BASE: usize = 0x80_000;

/// The `tick` time.
// FIXME: When you're ready, change this to something more reasonable.
pub const TICK: Duration = Duration::from_millis(1000);

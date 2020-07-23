use crate::console::kprintln;
use crate::mutex::Mutex;
use crate::param;
use crate::util::align_up;

use aarch64::*;

mod address;
mod pagetable;

pub use self::address::{PhysicalAddr, VirtualAddr};
pub use self::pagetable::*;
use crate::param::{VISOR_MASK_BITS, GUEST_MASK_BITS};

/// Thread-safe (locking) wrapper around a hypervisor page table.
pub struct VMManager(Mutex<Option<VisorPageTable>>);

impl VMManager {
    /// Returns an uninitialized `VMManager`.
    ///
    /// The virtual memory manager must be initialized by calling `initialize()` and `setup()`
    /// before the first memory allocation. Failure to do will result in panics.
    pub const fn uninitialized() -> Self {
        VMManager(Mutex::new(None))
    }

    /// Initializes the virtual memory manager.
    /// The caller should assure that the method is invoked only once during the hypervisor
    /// initialization.
    pub fn initialize(&self) {
        self.0.lock().replace(VisorPageTable::new());
        self.setup();
    }

    /// Set up the virtual memory manager.
    /// The caller should assure that `initialize()` has been called before calling this function.
    /// Sets proper configuration bits to MAIR_EL1, TCR_EL1, TTBR0_EL1, and TTBR1_EL1 registers.
    ///
    /// # Panics
    ///
    /// Panics if the current system does not support 64KB memory translation granule size.
    pub fn setup(&self) {
        let kern_page_table = self.0.lock();
        let baddr = kern_page_table.as_ref().unwrap().get_baddr().as_u64();
        
        unsafe {
            assert!(ID_AA64MMFR0_EL1.get_value(ID_AA64MMFR0_EL1::TGran64) == 0);
            
            let ips = ID_AA64MMFR0_EL1.get_value(ID_AA64MMFR0_EL1::PARange);

            MAIR_EL2.set(
                (0xFF <<  0) |// AttrIdx=0: normal, IWBWA, OWBWA, NTR
                (0x04 <<  8) |// AttrIdx=1: device, nGnRE (must be OSH too)
                (0x44 << 16), // AttrIdx=2: non cacheable
            );
            
            TCR_EL2.set(
                (0b1 << 31)  |// RES1
                (0b1 << 23)  |// RES1
                (0b00 << 20) |// TBI=0, no tagging
                (ips  << 16) |// IPS
                (0b01 << 14) |// TG0=64k
                (0b11 << 12) |// SH0=3 inner
                (0b01 << 10) |// ORGN1=1 write back
                (0b01 << 8)  |// IRGN1=1 write back
                ((VISOR_MASK_BITS as u64) << 0), // T0SZ=32 (4GB)
            );
            isb();

            TTBR0_EL2.set(baddr);
            asm!("dsb ish");
            isb();

            nuke_tlb_host();

            SCTLR_EL2.set(SCTLR_EL2.get() | SCTLR_EL2::I | SCTLR_EL2::C | SCTLR_EL2::M);
            asm!("dsb sy");
            isb();

            VTCR_EL2.set(
                (0b1 << 31)  |// RES1
                (ips  << 16) |// IPS
                (0b01 << 14) |// TG0=64k
                (0b11 << 12) |// SH0=3 inner
                (0b01 << 10) |// ORGN1=1 write back
                (0b01 << 8)  |// IRGN1=1 write back
                (0b01 << 6)  |// 64K granule translation starts at level 2
                ((GUEST_MASK_BITS as u64) << 0), // T0SZ=34 (1GB)
            );
            isb();

            HCR_EL2.set(HCR_EL2::VM | HCR_EL2::RES1);
            asm!("dsb sy");
            isb();
        }
    }

    /// Returns the base address of the hypervisor page table as `PhysicalAddr`.
    pub fn get_baddr(&self) -> PhysicalAddr {
        self.0.lock().as_ref().expect("VMM uninitialized").get_baddr()
    }

    // This is poor design; a better solution would be to make MutexGuard a monad
    pub fn critical<F: FnOnce(&mut VisorPageTable)>(&self, f: F) {
        f(self.0.lock().as_mut().expect("VMM uninitialized"));
    }

    // mark this page as non-cacheable for the visor
    pub fn mark_noncacheable<T: Sized>(&self, ptr: *const T) {
        let start = ptr as usize;
        let end = align_up(ptr as usize + core::mem::size_of::<T>(), param::PAGE_SIZE);
        let mut pt = self.0.lock();
        let pt = pt.as_mut().expect("VMM uninitialized");
        for page in (start..end).step_by(param::PAGE_SIZE) {
            pt.mark_noncacheable(VirtualAddr::from(page));
        }
    }
}

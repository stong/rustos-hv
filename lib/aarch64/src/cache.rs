/// Align `addr` downwards to the nearest multiple of `align`.
///
/// The returned usize is always <= `addr.`
///
/// # Panics
///
/// Panics if `align` is not a power of 2.
fn align_down(addr: usize, align: usize) -> usize {
    if align == 0 {
        panic!("align must be a power of two");
    }
    if align & (align - 1) != 0 {
        panic!("align must be a power of two");
    }
    let mask = align - 1;
    addr & !mask
}

/// Align `addr` upwards to the nearest multiple of `align`.
///
/// The returned `usize` is always >= `addr.`
///
/// # Panics
///
/// Panics if `align` is not a power of 2
/// or aligning up overflows the address.
fn align_up(addr: usize, align: usize) -> usize {
    align_down(addr + (align - 1), align) // parenthesis important to avoid overflowing!!!
}

pub fn clean_invalidate_dcache(mut addr: u64, length: u64) {
    unsafe {
        asm!("dmb sy" :::: "volatile");
        let mut end = addr + length;
        addr = align_down(addr as usize, 64) as u64;
        end = align_up(end as usize, 64) as u64;
        for i in (addr..end).step_by(64) {
            asm!("dc civac, $0" :: "r"(i) : "memory" : "volatile");
        }
        asm!("dsb sy; isb" :::: "volatile");
    }
}

pub fn clear_icache() {
    unsafe {
        asm!("dsb sy
            isb
            ic iallu
            isb" ::: "memory" : "volatile");
    }
}

// Flush ALL TLB, Stage 1 only
pub fn nuke_tlb_host() {
    unsafe {
        asm!("dsb sy
            tlbi alle2
            dsb sy
            isb"
            ::: "memory" : "volatile"
        );
    }
}

// Flush ALL TLB, Stage 1 & Stage 2
pub fn nuke_tlb_guest() {
    unsafe {
        asm!("dsb sy
            tlbi alle1
            dsb sy
            isb"
            ::: "memory" : "volatile"
        );
    }
}

// Flush TLB, Stage 1 & Stage 2, for current VMID
pub fn nuke_local_tlb_guest() {
    unsafe {
        asm!("dsb sy
            tlbi vmalls12e1
            dsb sy
            isb"
            ::: "memory" : "volatile"
        );
    }
}

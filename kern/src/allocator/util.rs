/// Align `addr` downwards to the nearest multiple of `align`.
///
/// The returned usize is always <= `addr.`
///
/// # Panics
///
/// Panics if `align` is not a power of 2.
pub fn align_down(addr: usize, align: usize) -> usize {
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
pub fn align_up(addr: usize, align: usize) -> usize {
    align_down(addr + (align - 1), align) // parenthesis important to avoid overflowing!!!
}

pub fn is_aligned(addr: usize, align: usize) -> bool {
    return addr & (align - 1) == 0;
}

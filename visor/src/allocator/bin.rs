use core::fmt::Debug;
use core::alloc::Layout;
use core::fmt;
use core::ptr;
use core::cmp::max;

use crate::allocator::linked_list::LinkedList;
use crate::util::{is_aligned, align_up};
use crate::allocator::LocalAlloc;

/// A simple allocator that allocates based on size classes.
///   bin 0 (2^3 bytes)    : handles allocations in (0, 2^3]
///   bin 1 (2^4 bytes)    : handles allocations in (2^3, 2^4]
///   ...
///   bin 29 (2^32 bytes): handles allocations in (2^31, 2^32]
///   
///   map_to_bin(size) -> k
///   

pub struct Allocator {
    bins: [LinkedList; 30],
    top_start: usize, top_end: usize // topchunk
}

fn size_to_bin(size: usize) -> usize {
    if size == 0 {
        panic!("allocation size must be nonzero")
    }
    (29 as usize).saturating_sub(((size - 1) as u32).leading_zeros() as usize)
}

fn bin_to_size(bin: usize) -> usize {
    if bin > 29 {
        panic!("invalid bin number")
    }
    1 << (bin + 3)
}

#[test]
fn test_size_to_bin() {
    for i in 1..=8 {
        assert_eq!(size_to_bin(i), 0);
    }
    assert_eq!(size_to_bin(9), 1);
    assert_eq!(size_to_bin(16), 1);
    assert_eq!(size_to_bin(17), 2);
    assert_eq!(size_to_bin(32), 2);
    assert_eq!(size_to_bin(1234), 8);

    assert_eq!(bin_to_size(0), 8);
    assert_eq!(bin_to_size(1), 16);
    assert_eq!(bin_to_size(2), 32);
    assert_eq!(bin_to_size(3), 64);

    assert_eq!(bin_to_size(size_to_bin(5)), 8);
    assert_eq!(bin_to_size(size_to_bin(15)), 16);
    assert_eq!(bin_to_size(size_to_bin(25)), 32);
}

impl Allocator {
    /// Creates a new bin allocator that will allocate memory from the region
    /// starting at address `start` and ending at address `end`.
    pub fn new(start: usize, end: usize) -> Allocator {
        Allocator{
            bins: [LinkedList::new(); 30],
            top_start: start,
            top_end: end
        }
    }

    fn insert_chunk(&mut self, ptr: usize, bin: usize) {
        if bin > 29 {
            panic!("invalid bin");
        }
        let size = bin_to_size(bin);
        if ptr + size == self.top_start {
            // coalesce into wilderness
            self.top_start = ptr;
            return;
        }
        if bin < 29 {
            // search for adjacent
            for node in self.bins[bin].iter_mut() {
                let other_ptr = node.value() as usize;
                if ptr + size == other_ptr { // merge right
                    node.pop();
                    return self.insert_chunk(ptr, bin + 1);
                } else if other_ptr + size == ptr { // merge left
                    node.pop();
                    return self.insert_chunk(other_ptr, bin + 1);
                }
            }
        }
        // no merge, insert.
        unsafe { self.bins[bin].push(ptr as *mut usize); }
    }

    // binary division of arbitrary-sized chunk into bins
    fn rebin(&mut self, mut ptr: usize, mut size: usize) {
        if size == 0 {
            return; // nothing to do
        }
        if !is_aligned(ptr, 8) || size < 8 || !is_aligned(size, 8) {
            panic!("invalid chunk")
        }
        let ptr_check = ptr + size;
        size >>= 3;
        let mut i = 0;
        while size != 0 {
            if size & 1 != 0 {
                self.insert_chunk(ptr, i);
                ptr += bin_to_size(i);
            }
            size >>= 1;
            i += 1;
        }
        assert_eq!(ptr, ptr_check);
    }
}

impl LocalAlloc for Allocator {
    /// Allocates memory. Returns a pointer meeting the size and alignment
    /// properties of `layout.size()` and `layout.align()`.
    ///
    /// If this method returns an `Ok(addr)`, `addr` will be non-null address
    /// pointing to a block of storage suitable for holding an instance of
    /// `layout`. In particular, the block will be at least `layout.size()`
    /// bytes large and will be aligned to `layout.align()`. The returned block
    /// of storage may or may not have its contents initialized or zeroed.
    ///
    /// # Safety
    ///
    /// The _caller_ must ensure that `layout.size() > 0` and that
    /// `layout.align()` is a power of two. Parameters not meeting these
    /// conditions may result in undefined behavior.
    ///
    /// # Errors
    ///
    /// Returning null pointer (`core::ptr::null_mut`)
    /// indicates that either memory is exhausted
    /// or `layout` does not meet this allocator's
    /// size or alignment constraints.
    unsafe fn alloc(&mut self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return ptr::null_mut();
        }

        let align = max(8, layout.align()); // clamp alignment up to 8, so the wasted space is at least big enough to be added to a bin freelist
        let size = bin_to_size(size_to_bin(layout.size())); // calculate bin size

        // check bins
        for i in (size_to_bin(size))..=29 {
            if let Some(node) = self.bins[i].iter_mut().find(|node| is_aligned(node.value() as usize, layout.align())) {
                let chunk = node.pop();
                self.rebin(chunk as usize + size, bin_to_size(i) - size);
                return chunk as *mut u8;
            }
        }
        
        // no bin, take from wilderness
        let chunk_start = align_up(self.top_start, align);
        self.rebin(self.top_start, chunk_start - self.top_start);
        self.top_start = chunk_start;
        if self.top_start + size > self.top_end {
            return ptr::null_mut(); // out of memory!
        }
        self.top_start += size;
        assert!(is_aligned(self.top_start, 8));
        chunk_start as *mut u8
    }

    /// Deallocates the memory referenced by `ptr`.
    ///
    /// # Safety
    ///
    /// The _caller_ must ensure the following:
    ///
    ///   * `ptr` must denote a block of memory currently allocated via this
    ///     allocator
    ///   * `layout` must properly represent the original layout used in the
    ///     allocation call that returned `ptr`
    ///
    /// Parameters not meeting these conditions may result in undefined
    /// behavior.
    unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        if layout.size() == 0 || ptr == ptr::null_mut() {
            return;
        }
        if !is_aligned(ptr as usize, layout.align()) {
            panic!("dealloc argument {:x} does not match alignment {}", ptr as usize, layout.align());
        }
        self.rebin(ptr as usize, bin_to_size(size_to_bin(layout.size())));
    }
}

impl Debug for Allocator {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "bin allocator {{ bins: {{")?;
        for (i, bin) in self.bins.iter().filter(|bin| !bin.is_empty()).enumerate() {
            write!(f, " {}: [", bin_to_size(i))?;
            for chunk in bin.iter() {
                write!(f, " {:x} ", chunk as usize)?;
            }
            write!(f, "];")?;
        }
        write!(f, " }} topchunk: {:x}:{:x}", self.top_start, self.top_end)
    }
}

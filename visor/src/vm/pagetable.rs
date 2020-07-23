use core::fmt::Debug;
use core::iter::Chain;
use core::ops::{Deref, DerefMut};
use core::slice::Iter;

use alloc::boxed::Box;
use alloc::fmt;
use core::alloc::{GlobalAlloc, Layout};

use crate::allocator;
use crate::param::*;
use crate::util::align_up;
use crate::vm::{PhysicalAddr, VirtualAddr};
use crate::ALLOCATOR;
use crate::VMM;

use aarch64::vmsa::*;
use shim::const_assert_size;

#[repr(C)]
pub struct Page([u8; PAGE_SIZE]);
const_assert_size!(Page, PAGE_SIZE);

impl Page {
    pub const SIZE: usize = PAGE_SIZE;
    pub const ALIGN: usize = PAGE_SIZE;

    fn layout() -> Layout {
        unsafe { Layout::from_size_align_unchecked(Self::SIZE, Self::ALIGN) }
    }
}

#[repr(C)]
#[repr(align(65536))]
pub struct L2PageTable {
    pub entries: [RawEntry; 8192],
}
const_assert_size!(L2PageTable, PAGE_SIZE);

impl L2PageTable {
    /// Returns a new `L2PageTable`
    fn new() -> L2PageTable {
        L2PageTable{
            entries: [RawEntry::new(0); 8192]
        }
    }

    /// Returns a `PhysicalAddr` of the pagetable.
    pub fn as_ptr(&self) -> PhysicalAddr {
        PhysicalAddr::from(self.entries.as_ptr() as u64)
    }
}

#[derive(Copy, Clone)]
pub struct L3Entry(RawEntry);

impl L3Entry {
    /// Returns a new `L3Entry`.
    fn new() -> L3Entry {
        L3Entry(RawEntry::new(0))
    }

    /// Returns `true` if the L3Entry is valid and `false` otherwise.
    pub fn is_valid(&self) -> bool {
        self.0.get_value(RawEntry::VALID) == 1
    }

    /// Extracts `ADDR` field of the L3Entry and returns as a `PhysicalAddr`
    /// if valid. Otherwise, return `None`.
    fn get_page_addr(&self) -> Option<PhysicalAddr> {
        if self.is_valid() {
            Some(PhysicalAddr::from(self.0.get_value(RawEntry::ADDR)))
        } else {
            None
        }
    }
}

#[repr(C)]
#[repr(align(65536))]
pub struct L3PageTable {
    pub entries: [L3Entry; 8192],
}
const_assert_size!(L3PageTable, PAGE_SIZE);

impl L3PageTable {
    /// Returns a new `L3PageTable`.
    fn new() -> L3PageTable {
        L3PageTable {
            entries: [L3Entry::new(); 8192]
        }
    }

    /// Returns a `PhysicalAddr` of the pagetable.
    pub fn as_ptr(&self) -> PhysicalAddr {
        PhysicalAddr::from(self.entries.as_ptr() as u64)
    }
}

#[repr(C)]
#[repr(align(65536))]
pub struct PageTable {
    pub l2: L2PageTable,
    pub l3: [L3PageTable; 2],
}

impl PageTable {
    fn  new_l2pte(l3pt: &L3PageTable, perm: u64) -> RawEntry {
        let mut pte = RawEntry::new(0);
        pte.set_value(l3pt.as_ptr().as_u64() >> PAGE_ALIGN, RawEntry::ADDR);
        pte.set_value(1, RawEntry::VALID); // valid
        pte.set_value(1, RawEntry::TYPE); // pointer to next level translation table
        pte.set_value(0b000, RawEntry::ATTR); // normal memory
        pte.set_value(perm, RawEntry::AP); // permissions
        pte.set_value(0b11, RawEntry::SH); // regular memory should be inner shareable
        pte.set_value(1, RawEntry::AF); // we assume all pages are being used
        pte
    }

    /// Returns a new `Box` containing `PageTable`.
    /// Entries in L2PageTable should be initialized properly before return.
    fn new(perm: u64) -> Box<PageTable> {
        let mut pt = Box::new(PageTable {
            l2: L2PageTable::new(),
            l3: [L3PageTable::new(), L3PageTable::new()]
        });
        pt.l2.entries[0] = Self::new_l2pte(&pt.l3[0], perm);
        pt.l2.entries[1] = Self::new_l2pte(&pt.l3[1], perm);
        pt
    }

    fn  new_l2pte_stage2(l3pt: &L3PageTable, perm: u64) -> RawStage2Entry {
        let mut pte = RawStage2Entry::new(0);
        pte.set_value(l3pt.as_ptr().as_u64() >> PAGE_ALIGN, RawStage2Entry::ADDR);
        pte.set_value(1, RawStage2Entry::VALID); // valid
        pte.set_value(1, RawStage2Entry::TYPE); // pointer to next level translation table
        pte.set_value(0b11, RawStage2Entry::CACHE); // normal memory, outer write-back cacheable (ref D4.5.2)
        pte.set_value(0b11, RawStage2Entry::ATTR); // inner write-back cacheable
        pte.set_value(perm, RawStage2Entry::S2AP); // permissions
        pte.set_value(0b11, RawStage2Entry::SH); // regular memory should be inner shareable
        pte.set_value(1, RawStage2Entry::AF); // we assume all pages are being used
        pte
    }

    fn new_stage2(perm: u64) -> Box<PageTable> {
        let mut pt = Box::new(PageTable {
            l2: L2PageTable::new(),
            l3: [L3PageTable::new(), L3PageTable::new()]
        });
        pt.l2.entries[0] = RawEntry::new(Self::new_l2pte_stage2(&pt.l3[0], perm).get());
        pt.l2.entries[1] = RawEntry::new(Self::new_l2pte_stage2(&pt.l3[1], perm).get());
        pt
    }

    /// Returns the (L2index, L3index) extracted from the given virtual address.
    /// Since we are only supporting 1GB virtual memory in this system, L2index
    /// should be smaller than 2.
    ///
    /// # Panics
    ///
    /// Panics if the virtual address is not properly aligned to page size.
    /// Panics if extracted L2index exceeds the number of L3PageTable.
    fn locate(va: VirtualAddr) -> (usize, usize) {
        let addr = va.as_usize();
        if addr & (PAGE_SIZE - 1) != 0 {
            panic!("Virtual address not aligned to page boundary")
        }
        let l3index: usize = (addr >> PAGE_ALIGN) & ((1 << 13) - 1);
        let l2index: usize = (addr >> 29) & ((1 << 13) - 1);
        if l2index >= 2 {
            panic!("L2 index exceeds number of L3 page table")
        }
        (l2index, l3index)
    }

    /// Returns `true` if the L3entry indicated by the given virtual address is valid.
    /// Otherwise, `false` is returned.
    pub fn is_valid(&self, va: VirtualAddr) -> bool {
        let addr = va.as_usize();
        let l3index: usize = (addr >> PAGE_ALIGN) & ((1 << 13) - 1);
        l3index < 8192
    }

    /// Returns `true` if the L3entry indicated by the given virtual address is invalid.
    /// Otherwise, `true` is returned.
    pub fn is_invalid(&self, va: VirtualAddr) -> bool {
        !self.is_valid(va)
    }

    /// Set the given RawEntry `entry` to the L3Entry indicated by the given virtual
    /// address.
    pub fn set_entry(&mut self, va: VirtualAddr, entry: RawEntry) -> &mut Self {
        use crate::console::{kprintln};
        let (l2index, l3index) = Self::locate(va);
        self.l3[l2index].entries[l3index] = L3Entry(entry);
        self
    }

    pub fn get_entry(&mut self, va: VirtualAddr) -> &mut L3Entry {
        let (l2index, l3index) = Self::locate(va);
        &mut self.l3[l2index].entries[l3index]
    }

    /// Returns a base address of the pagetable. The returned `PhysicalAddr` value
    /// will point the start address of the L2PageTable.
    pub fn get_baddr(&self) -> PhysicalAddr {
        self.l2.as_ptr()
    }
}

impl<'a> IntoIterator for &'a PageTable {
    type Item = &'a L3Entry;
    type IntoIter = core::iter::Chain<core::slice::Iter<'a, L3Entry>, core::slice::Iter<'a, L3Entry>>;

    fn into_iter(self) -> Self::IntoIter {
        self.l3[0].entries.into_iter().chain(self.l3[1].entries.into_iter())
    }
}

pub struct VisorPageTable(Box<PageTable>);

impl VisorPageTable {
    fn new_l3pte(va: PhysicalAddr, device: bool) -> RawEntry {
        let mut pte = RawEntry::new(0);
        pte.set_value(va.as_u64() >> PAGE_ALIGN, RawEntry::ADDR);
        pte.set_value(1, RawEntry::VALID); // valid
        pte.set_value(1, RawEntry::TYPE); // valid
        pte.set_value(if device { 0b001 } else { 0b000 }, RawEntry::ATTR); // device or normal memory. see MAIR_EL2 in vm.rs
        pte.set_value(EntryPerm::KERN_RW, RawEntry::AP); // kernel R/W
        pte.set_value(if device { 0b10 } else { 0b11 }, RawEntry::SH); // outer shareable if device, inner shareable otherwise
        pte.set_value(1, RawEntry::AF); // we assume all pages are being used
        pte
    }

    /// Returns a new `VisorPageTable`. `VisorPageTable` should have a `Pagetable`
    /// created with `KERN_RW` permission.
    ///
    /// Set L3entry of ARM physical address starting at 0x00000000 for RAM and
    /// physical address range from `IO_BASE` to `IO_BASE_END` for peripherals.
    /// Each L3 entry should have correct value for lower attributes[10:0] as well
    /// as address[47:16]. Refer to the definition of `RawEntry` in `vmsa.rs` for
    /// more details.
    pub fn new() -> VisorPageTable {
        let mut pt = PageTable::new(EntryPerm::KERN_RW); // kernel R/W
        // fill in address space
        let (_ , mut end) = allocator::memory_map().expect("memory_map");
        end = align_up(end, PAGE_SIZE);
        for addr in (0..end).step_by(PAGE_SIZE) {
            pt.set_entry(VirtualAddr::from(addr), Self::new_l3pte(PhysicalAddr::from(addr), false));
        }
        for addr in (IO_BASE..IO_BASE_END).step_by(PAGE_SIZE) {
            pt.set_entry(VirtualAddr::from(addr), Self::new_l3pte(PhysicalAddr::from(addr), true));
        }
        VisorPageTable(pt)
    }

    pub fn mark_noncacheable(&mut self, page: VirtualAddr) {
        self.get_entry(page).0.set_value(0b010, RawEntry::ATTR);
    }
}

pub enum PagePerm {
    RW,
    RO,
    RWX,
}

pub struct GuestPageTable(Box<PageTable>);

impl GuestPageTable {
    /// Returns a new `GuestPageTable` containing a `PageTable` created with
    /// `READWRITE` permission.
    pub fn new() -> GuestPageTable {
        let mut pt = PageTable::new_stage2(Stage2EntryPerm::READWRITE); // user RW
        
        // for now just pass through the MMIO
        // for addr in (IO_BASE as u64..IO_BASE_END as u64).step_by(PAGE_SIZE) {
        //     let mut pte = RawStage2Entry::new(0);
        //     pte.set_value(addr >> PAGE_ALIGN, RawStage2Entry::ADDR);
        //     pte.set_value(1, RawStage2Entry::VALID); // valid
        //     pte.set_value(1, RawStage2Entry::TYPE); // valid
        //     pte.set_value(0b00, RawStage2Entry::CACHE); // device memory. Ref D4.5.2
        //     pte.set_value(0b01, RawStage2Entry::ATTR); // Region is Device-nGnRE memory (no-gathering, no-reorder, early-acknowledge). I don't know what this means. Ref A53 6.3
        //     pte.set_value(Stage2EntryPerm::READWRITE, RawStage2Entry::S2AP); // R/W
        //     pte.set_value(0b10, RawStage2Entry::SH); // outer shareable
        //     pte.set_value(1, RawStage2Entry::AF); // we assume all pages are being used
        //     pt.set_entry(VirtualAddr::from(addr), RawEntry::new(pte.get()));
        // }

        // do NOT cache pagetables in hypervisor memory, or else we will need to flush every time we edit them, as it may cause incoherency with the TLB
        VMM.mark_noncacheable(&pt);
        
        GuestPageTable(pt)
    }

    /// Allocates a page and set an L3 entry translates given virtual address to the
    /// physical address of the allocated page. Returns the allocated page.
    ///
    /// # Panics
    /// Panics if the virtual address is lower than `GUEST_IMG_BASE`.
    /// Panics if the virtual address has already been allocated.
    /// Panics if allocator fails to allocate a page.
    ///
    /// TODO. use Result<T> and make it failurable
    /// TODO. use perm properly
    pub fn alloc(&mut self, va: VirtualAddr, _perm: PagePerm) -> &mut [u8] {
        use core::alloc::GlobalAlloc;
        // todo: mark allocate pages are NC for visor
        let buf = unsafe { ALLOCATOR.alloc(Page::layout()) };
        if buf as usize == 0 {
            panic!("failed to allocate user page");
        }
        if self.get_entry(va).is_valid() {
            panic!("page is already allocated")
        }

        let mut pte = RawStage2Entry::new(0);
        pte.set_value(buf as u64 >> PAGE_ALIGN, RawStage2Entry::ADDR);
        pte.set_value(1, RawStage2Entry::VALID); // valid
        pte.set_value(1, RawStage2Entry::TYPE); // valid
        pte.set_value(0b11, RawStage2Entry::CACHE); // normal memory, outer write-back cacheable
        pte.set_value(0b11, RawStage2Entry::ATTR); // inner write-back cacheable
        pte.set_value(Stage2EntryPerm::READWRITE, RawStage2Entry::S2AP); // R/W
        pte.set_value(0b11, RawStage2Entry::SH); // inner shareable
        pte.set_value(1, RawStage2Entry::AF); // we don't need AF yet
        self.set_entry(va, RawEntry::new(pte.get()));

        VMM.mark_noncacheable(buf as *const Page);
        unsafe { core::slice::from_raw_parts_mut(buf, PAGE_SIZE) }
    }
}

impl Deref for VisorPageTable {
    type Target = PageTable;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Deref for GuestPageTable {
    type Target = PageTable;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for VisorPageTable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl DerefMut for GuestPageTable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for GuestPageTable {
    fn drop(&mut self) {
        use core::alloc::GlobalAlloc;
        for pte in self.into_iter() {
            if pte.0.get() != 0 {
                let page = (pte.0.get_value(RawStage2Entry::ADDR) << PAGE_ALIGN) as *mut u8;
                unsafe { ALLOCATOR.dealloc(page, Page::layout()) };
            }
        }
    }
}

impl Debug for GuestPageTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "guest page table at {:x}", self.get_baddr().as_usize())?;
        Ok(())
    }
}

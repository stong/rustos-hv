use alloc::boxed::Box;
use shim::io;
use shim::path::Path;

use aarch64;

use crate::param::*;
use crate::process::{Stack, State};
use crate::traps::TrapFrame;
use crate::vm::*;
use kernel_api::{OsError, OsResult};

/// Type alias for the type of a process ID.
pub type Id = u8;

/// A structure that represents the complete state of a process.
#[derive(Debug)]
pub struct Process {
    /// The saved trap frame of a process.
    pub context: Box<TrapFrame>,
    /// The page table describing the Virtual Memory of the process
    pub vmap: Box<GuestPageTable>,
    /// The scheduling state of the process.
    pub state: State,
}

impl Process {
    /// Creates a new process with a zeroed `TrapFrame` (the default), a zeroed
    /// stack of the default size, and a state of `Ready`.
    ///
    /// If enough memory could not be allocated to start the process, returns
    /// `None`. Otherwise returns `Some` of the new `Process`.
    pub fn new() -> OsResult<Process> {
        let vmap = Box::new(GuestPageTable::new());
        let mut tf = TrapFrame::default();
        tf.VTTBR = vmap.get_baddr().as_u64();
        tf.VBAR_EL1 = 0x1DEAD0000; // avoid exception looping. just set this to an nontranslatable address if the kernel crashes before setting up its handler
        tf.SCTLR_EL1 = aarch64::SCTLR_EL1::RES1;
        Ok(Process{
            context: Box::new(tf),
            vmap: vmap,
            state: State::Ready,
        })
    }

    pub fn set_vmid(&mut self, vmid: Id) {
        self.context.VTTBR = (self.context.VTTBR & 0x0000FFFFFFFFFFFF) | ((vmid as u64) << 48);
    }

    pub fn get_vmid(&self) -> Id {
        aarch64::VTTBR_EL2::get_masked(self.context.VTTBR, aarch64::VTTBR_EL2::VMID) as Id
    }

    /// Load a program stored in the given path by calling `do_load()` method.
    /// Set trapframe `context` corresponding to the its page table.
    ///
    /// Returns Os Error if do_load fails.
    pub fn load<P: AsRef<Path>>(pn: P) -> OsResult<Process> {
        use crate::VMM;

        let mut p = Process::do_load(pn)?;

        // flush dcache of guest pagetable so we are sure that future translations will see our new pagetable.
        // aarch64::clean_invalidate_dcache(p.vmap.get_baddr().as_u64(), core::mem::size_of::<PageTable>() as u64);

        p.context.ELR = Process::get_image_base().as_u64();
        // guest expects interrupts to be masked
        p.context.SPSR_EL1 = aarch64::SPSR_EL1::F | aarch64::SPSR_EL1::A | aarch64::SPSR_EL1::I | aarch64::SPSR_EL1::D;

        Ok(p)
    }

    /// Creates a process and open a file with given path.
    /// Allocates one page for stack with read/write permission, and N pages with read/write/execute
    /// permission to load file's contents.
    fn do_load<P: AsRef<Path>>(pn: P) -> OsResult<Process> {
        use crate::FILESYSTEM;
        use fat32::traits::FileSystem;
        use io::Read;

        let mut p = Process::new()?;

        let mut va = VirtualAddr::from(0);
        let null_page = p.vmap.alloc(VirtualAddr::from(va), PagePerm::RWX);
        va += VirtualAddr::from(PAGE_SIZE);
        // setup atags
        // Core(Core { flags: 1, page_size: 4096, root_dev: 0 })
        use pi::atags::raw;
        use pi::atags::{Atag, ATAG_BASE};
        let core = raw::Atag{
            dwords: 5,
            tag: raw::Atag::CORE,
            kind: raw::Kind{core: raw::Core{ flags: 1, page_size: 4096, root_dev: 0 }}
        };
        // Mem(Mem { size: GUEST_MAX_VM_SIZE, start: 0 })
        let mem = raw::Atag{
            dwords: 4,
            tag: raw::Atag::MEM,
            kind: raw::Kind{mem: raw::Mem { size: GUEST_MAX_VM_SIZE as u32, start: 0 }}
        };
        // None
        let end = raw::Atag{
            dwords: 0,
            tag: raw::Atag::NONE,
            kind: raw::Kind{none: raw::None{}}
        };
        assert!(ATAG_BASE < PAGE_SIZE); // assert ATAG_BASE in first page
        unsafe {
            let mut ptr = &mut null_page[ATAG_BASE] as *mut u8 as *mut raw::Atag;
            *ptr = core;
            ptr = (ptr as *mut u32).offset((*ptr).dwords as isize) as *mut raw::Atag;
            *ptr = mem;
            ptr = (ptr as *mut u32).offset((*ptr).dwords as isize) as *mut raw::Atag;
            *ptr = end;
        }
    
        // 0x10000..kern_base
        while va.as_u64() < KERN_START_ADDR {
            p.vmap.alloc(VirtualAddr::from(va), PagePerm::RWX);
            va += VirtualAddr::from(PAGE_SIZE);
        }
    
        // load image
        let mut file = FILESYSTEM.open_file(pn)?;
        'outer: loop {
            let page = p.vmap.alloc(va, PagePerm::RWX);
            va += VirtualAddr::from(PAGE_SIZE);
            let mut n = 0;
            while n < PAGE_SIZE {
                let nread = file.read(&mut page[n..])?;
                if nread == 0 {
                    break 'outer;
                }
                n += nread;
            }
        }
        
        Ok(p)
    }

    /// Returns the highest `VirtualAddr` that is supported by this system.
    pub fn get_ipa_max() -> VirtualAddr {
        VirtualAddr::from(GUEST_MAX_VM_SIZE)
    }

    /// Returns the `VirtualAddr` represents the base address of the user
    /// memory space.
    pub fn get_image_base() -> VirtualAddr {
        VirtualAddr::from(KERN_START_ADDR)
    }

    /// Returns `true` if this process is ready to be scheduled.
    ///
    /// This functions returns `true` only if one of the following holds:
    ///
    ///   * The state is currently `Ready`.
    ///
    ///   * An event being waited for has arrived.
    ///
    ///     If the process is currently waiting, the corresponding event
    ///     function is polled to determine if the event being waiting for has
    ///     occured. If it has, the state is switched to `Ready` and this
    ///     function returns `true`.
    ///
    /// Returns `false` in all other cases.
    pub fn is_ready(&mut self) -> bool {
        let mut state = core::mem::replace(&mut self.state, State::Ready);
        let result = match state {
            State::Ready => true,
            State::Waiting(ref mut func) => func(self),
            _ => false
        };
        core::mem::replace(&mut self.state, state);
        result
    }
}

use core::ops::DerefMut;
use alloc::boxed::Box;
use alloc::collections::vec_deque::VecDeque;
use core::fmt;

use pi::timer;
use pi::interrupt::{Controller, Interrupt};
use aarch64::*;

use crate::param;
use crate::mutex::{Mutex, MutexFunctor};
use crate::param::{PAGE_MASK, PAGE_SIZE, TICK};
use crate::process::{Id, Process, State};
use crate::traps::TrapFrame;
use crate::VMM;
use crate::IRQ;

/// Process scheduler for the entire machine.
#[derive(Debug)]
pub struct GlobalScheduler(Mutex<Option<Scheduler>>);

impl GlobalScheduler {
    /// Returns an uninitialized wrapper around a local scheduler.
    pub const fn uninitialized() -> GlobalScheduler {
        GlobalScheduler(Mutex::new(None))
    }

    /// Enter a critical region and execute the provided closure with the
    /// internal scheduler.
    pub fn critical<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Scheduler) -> R,
    {
        let mut guard = self.0.lock();
        f(guard.as_mut().expect("scheduler uninitialized"))
    }

    pub fn expect(&'_ self) -> impl MutexFunctor<'_, Scheduler> + '_
    {
        self.0.lock().map(|opt| opt.as_mut().expect("scheduler uninitialized") )
    }

    pub fn get_by_vmid(&'_ self, vmid: u8) -> impl MutexFunctor<Process> + '_ {
        self.expect().map(|scheduler| scheduler.get_by_vmid(vmid).expect("bad vmid"))
    }

    /// Adds a process to the scheduler's queue and returns that process's ID.
    /// For more details, see the documentation on `Scheduler::add()`.
    pub fn add(&self, process: Process) -> Id {
        self.critical(move |scheduler| scheduler.add(process))
    }

    /// Performs a context switch using `tf` by setting the state of the current
    /// process to `new_state`, saving `tf` into the current process, and
    /// restoring the next process's trap frame into `tf`. For more details, see
    /// the documentation on `Scheduler::schedule_out()` and `Scheduler::switch_to()`.
    pub fn switch(&self, new_state: State, tf: &mut TrapFrame) -> Id {
        self.critical(|scheduler| scheduler.schedule_out(new_state, tf));
        self.switch_to(tf)
    }

    pub fn switch_to(&self, tf: &mut TrapFrame) -> Id {
        loop {
            let rtn = self.critical(|scheduler| scheduler.switch_to(tf));
            if let Some(id) = rtn {
                return id;
            }
            aarch64::wfe();
        }
    }

    /// Kills currently running process and returns that process's ID.
    /// For more details, see the documentaion on `Scheduler::kill()`.
    #[must_use]
    pub fn kill(&self, tf: &mut TrapFrame) -> Option<Id> {
        self.critical(|scheduler| scheduler.kill(tf))
    }

    /// Starts executing processes in user space using timer interrupt based
    /// preemptive scheduling. This method should not return under normal conditions.
    pub fn start(&self) -> ! {
        // schedule a timer interrupt 1 timeslice from now
        IRQ.register(Interrupt::Timer1, Box::new(|tf| {
            crate::console::kprintln!("Tick!");
            timer::tick_in(TICK);
            // SCHEDULER.switch(State::Ready, tf);
        }));
        timer::tick_in(TICK);
        let mut controller = Controller::new();
        controller.enable(Interrupt::Timer1);

        unsafe {
            // enable CNTP for EL1/EL0 (ref: D7.5.2, D7.5.13)
            // NOTE: This doesn't actually enable the counter stream.
            // CNTHCTL_EL2.set(CNTHCTL_EL2.get() | CNTHCTL_EL2::EL0VCTEN | CNTHCTL_EL2::EL0PCTEN);
            CNTVOFF_EL2.set(0);

            // enable AArch64 in EL1 (A53: 4.3.36)
            HCR_EL2.set(HCR_EL2.get() | HCR_EL2::RW | HCR_EL2::IMO | HCR_EL2::RES1);

            // enable floating point and SVE (SIMD) (A53: 4.3.38, 4.3.34)
            CPTR_EL2.set(0);
            CPACR_EL1.set(CPACR_EL1.get() | (0b11 << 20));

            // mask interrupts
            // DAIF.set(DAIF.get() | DAIF::D | DAIF::A | DAIF::I | DAIF::F);

            // target execution level EL1 (ref: C5.2.19)
            SPSR_EL2.set(
                SPSR_EL2::M & 0b0101 // EL1h
                // | SPSR_EL2::F
                // | SPSR_EL2::I
                // | SPSR_EL2::D
                // | SPSR_EL2::A,
            );
        }
        
        let process = self.get_by_vmid(0);

        // flush pagetables from dcache
        aarch64::clean_invalidate_dcache(process.vmap.get_baddr().as_u64(), core::mem::size_of::<crate::vm::PageTable>() as u64);
        // flush icache
        aarch64::clear_icache();
        // flush tlb
        aarch64::nuke_tlb_guest();

        crate::console::kprintln!("Switching to kernel NOW!");
        unsafe { 
            SP.set(&*process.context as *const TrapFrame as usize);
            asm!("bl context_restore;
            ldp x28, x29, [SP], #16;
            ldp lr, xzr, [SP], #16;
            mov SP, $0;
            eret;" :: "i"(param::KERN_STACK_BASE) :: "volatile");
        }
        unreachable!("bruh moment");
    }

    /// Initializes the scheduler and add userspace processes to the Scheduler
    pub unsafe fn initialize(&self) {
        let mut scheduler = Scheduler::new();
        let kernel = Process::load("/kernel.bin").expect("load failed");
        scheduler.add(kernel);
        self.0.lock().replace(scheduler);
    }

    // The following method may be useful for testing Phase 3:
    //
    // * A method to load a extern function to the user process's page table.
    //
    // pub fn test_phase_3(&self, proc: &mut Process){
    //     use crate::vm::{VirtualAddr, PagePerm};
    //
    //     let mut page = proc.vmap.alloc(
    //         VirtualAddr::from(GUEST_IMG_BASE as u64), PagePerm::RWX);
    //
    //     let text = unsafe {
    //         core::slice::from_raw_parts(test_user_process as *const u8, 24)
    //     };
    //
    //     page[0..24].copy_from_slice(text);
    // }
}

#[derive(Debug)]
pub struct Scheduler {
    processes: VecDeque<Process>,
    last_id: Id
}

impl Scheduler {
    /// Returns a new `Scheduler` with an empty queue.
    fn new() -> Scheduler {
        Scheduler {
            processes: VecDeque::new(),
            last_id: 0
        }
    }

    fn get_by_vmid(&mut self, vmid: u8) -> Option<&mut Process> {
        return Some(&mut self.processes[0]) // TODO: actually implement this lol
    }

    /// Adds a process to the scheduler's queue and returns that process's ID if
    /// a new process can be scheduled. The process ID is newly allocated for
    /// the process and saved in its `trap_frame`. If no further processes can
    /// be scheduled, returns `None`.
    ///
    /// It is the caller's responsibility to ensure that the first time `switch`
    /// is called, that process is executing on the CPU.
    fn add(&mut self, mut process: Process) -> Id {
        let vmid = self.last_id;
        process.set_vmid(vmid);
        self.processes.push_back(process);
        self.last_id = self.last_id.checked_add(1).expect("too many vmids");
        vmid
    }

    /// Finds the currently running process, sets the current process's state
    /// to `new_state`, prepares the context switch on `tf` by saving `tf`
    /// into the current process, and push the current process back to the
    /// end of `processes` queue.
    ///
    /// If the `processes` queue is empty or there is no current process,
    /// returns `false`. Otherwise, returns `true`.
    fn schedule_out(&mut self, new_state: State, tf: &mut TrapFrame) -> bool {
        unimplemented!("Scheduler::schedule_out()")
    }

    /// Finds the next process to switch to, brings the next process to the
    /// front of the `processes` queue, changes the next process's state to
    /// `Running`, and performs context switch by restoring the next process`s
    /// trap frame into `tf`.
    ///
    /// If there is no process to switch to, returns `None`. Otherwise, returns
    /// `Some` of the next process`s process ID.
    fn switch_to(&mut self, tf: &mut TrapFrame) -> Option<Id> {
        unimplemented!("Scheduler::switch_to()")
    }

    /// Kills currently running process by scheduling out the current process
    /// as `Dead` state. Removes the dead process from the queue, drop the
    /// dead process's instance, and returns the dead process's process ID.
    fn kill(&mut self, tf: &mut TrapFrame) -> Option<Id> {
        unimplemented!("Scheduler::kill()")
    }
}

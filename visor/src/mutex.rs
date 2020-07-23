use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::cell::UnsafeCell;
use core::ops::{DerefMut, Deref, Drop};

#[repr(align(32))]
pub struct RawMutex {
    lock: AtomicBool,
    owner: AtomicUsize
}

impl RawMutex {
    // Once MMU/cache is enabled, do the right thing here. For now, we don't
    // need any real synchronization.
    #[inline(never)]
    pub fn try_lock(&self) -> bool {
        let this = 0;
        if !self.lock.load(Ordering::Relaxed) || self.owner.load(Ordering::Relaxed) == this {
            self.lock.store(true, Ordering::Relaxed);
            self.owner.store(this, Ordering::Relaxed);
            true
        } else {
            false
        }
    }
    
    #[inline]
    fn unlock(&self) {
        self.lock.store(false, Ordering::Relaxed);
    }
}

#[repr(align(32))]
pub struct Mutex<T> {
    data: UnsafeCell<T>,
    raw: RawMutex
}

unsafe impl<T: Send> Send for Mutex<T> { }
unsafe impl<T: Send> Sync for Mutex<T> { }

pub struct MutexGuard<'a, T: 'a> {
    lock: &'a Mutex<T>
}

impl<'a, T> !Send for MutexGuard<'a, T> { }
unsafe impl<'a, T: Sync> Sync for MutexGuard<'a, T> { }

impl<T> Mutex<T> {
    pub const fn new(val: T) -> Mutex<T> {
        Mutex {
            data: UnsafeCell::new(val),
            raw: RawMutex {
                lock: AtomicBool::new(false),
                owner: AtomicUsize::new(usize::max_value())
            }
        }
    }
}

impl<T> Mutex<T> {
    // Once MMU/cache is enabled, do the right thing here. For now, we don't
    // need any real synchronization.
    #[inline]
    pub fn try_lock(&self) -> Option<MutexGuard<T>> {
        let this = 0;
        if self.raw.try_lock() {
            Some(MutexGuard { lock: &self })
        } else {
            None
        }
    }
    
    #[inline]
    pub fn lock(&self) -> MutexGuard<T> {
        // Wait until we can "aquire" the lock, then "acquire" it.
        loop {
            match self.try_lock() {
                Some(guard) => return guard,
                None => continue
            }
        }
    }
    
    #[inline]
    fn unlock(&self) {
        self.raw.unlock()
    }
}

pub trait MutexFunctor<'a, T: 'a> : DerefMut<Target = T> {
    fn map<U, F: FnOnce(&'a mut T) -> &'a mut U>(self, f: F) -> MappedMutexGuard<'a, U>;
}

impl<'a, T: 'a> MutexFunctor<'a, T> for MutexGuard<'a, T> {
    #[inline]
    fn map<U, F: FnOnce(&'a mut T) -> &'a mut U>(self, f: F) -> MappedMutexGuard<'a, U>
    {
        let raw = &self.lock.raw;
        let data = f(unsafe { &mut *self.lock.data.get() });
        core::mem::forget(self);
        MappedMutexGuard {
            data,
            raw,
        }
    }
}

impl<'a, T: 'a> Deref for MutexGuard<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        unsafe { & *self.lock.data.get() }
    }
}

impl<'a, T: 'a> DerefMut for MutexGuard<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<'a, T: 'a> Drop for MutexGuard<'a, T> {
    #[inline]
    fn drop(&mut self) {
        self.lock.unlock()
    }
}

impl<T: fmt::Debug> fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.try_lock() {
            Some(guard) => f.debug_struct("Mutex").field("data", &&*guard).finish(),
            None => f.debug_struct("Mutex").field("data", &"<locked>").finish()
        }
    }
}

pub struct MappedMutexGuard<'a, T: 'a> {
    data: *mut T,
    raw: &'a RawMutex
}

impl<'a, T> !Send for MappedMutexGuard<'a, T> { }
unsafe impl<'a, T: Sync> Sync for MappedMutexGuard<'a, T> { }

impl<'a, T: 'a> MutexFunctor<'a , T> for MappedMutexGuard<'a, T> {
    #[inline]
    fn map<U, F: FnOnce(&'a mut T) -> &'a mut U>(self, f: F) -> MappedMutexGuard<'a, U>
    {
        let raw = self.raw;
        let data = f(unsafe { &mut *self.data });
        core::mem::forget(self);
        MappedMutexGuard {
            data,
            raw: raw,
        }
    }
}

impl<'a, T: 'a> Deref for MappedMutexGuard<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        unsafe { & *self.data }
    }
}

impl<'a, T: 'a> DerefMut for MappedMutexGuard<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.data }
    }
}

impl<'a, T: 'a> Drop for MappedMutexGuard<'a, T> {
    #[inline]
    fn drop(&mut self) {
        self.raw.unlock()
    }
}

pub struct ReentrantLock (AtomicUsize);

impl ReentrantLock {
    pub const fn new() -> Self {
        ReentrantLock(AtomicUsize::new(0))
    }

    pub fn enter(&self) -> ReentrancyGuard {
        if self.0.fetch_add(1, Ordering::Relaxed) > 0 {
            panic!("Re-entrance detected! (nested fault?)")
        }
        ReentrancyGuard { lock: self }
    }
    
    fn leave(&self) {
        if self.0.fetch_sub(1, Ordering::Relaxed) == 0 {
            panic!("Double return?!")
        }
    }
}

pub struct ReentrancyGuard<'a> {
    lock: &'a ReentrantLock
}

impl<'a> Drop for ReentrancyGuard<'a> {
    fn drop(&mut self) {
        self.lock.leave();
    }
}

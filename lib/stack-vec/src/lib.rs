#![no_std]

#[cfg(test)]
mod tests;

use core::iter::IntoIterator;
use core::ops::{Deref, DerefMut};
use core::slice;

/// A contiguous array type backed by a slice.
///
/// `StackVec`'s functionality is similar to that of `std::Vec`. You can `push`
/// and `pop` and iterate over the vector. Unlike `Vec`, however, `StackVec`
/// requires no memory allocation as it is backed by a user-supplied slice. As a
/// result, `StackVec`'s capacity is _bounded_ by the user-supplied slice. This
/// results in `push` being fallible: if `push` is called when the vector is
/// full, an `Err` is returned.
#[derive(Debug)]
pub struct StackVec<'a, T: 'a> {
    // i think the bound here is needed as otherwise the T's lifetime may be shorter
    // and can lead to lifetime safety issue later on if the value dies and the
    // storage becomes a dangling reference
    storage: &'a mut [T],
    cap: usize,
    len: usize
}

impl<'a, T: 'a> StackVec<'a, T> {
    /// Constructs a new, empty `StackVec<T>` using `storage` as the backing
    /// store. The returned `StackVec` will be able to hold `storage.len()`
    /// values.
    pub fn new(storage: &'a mut [T]) -> StackVec<'a, T> {
        StackVec::with_len(storage, 0)
    }

    /// Constructs a new `StackVec<T>` using `storage` as the backing store. The
    /// first `len` elements of `storage` are treated as if they were `push`ed
    /// onto `self.` The returned `StackVec` will be able to hold a total of
    /// `storage.len()` values.
    ///
    /// # Panics
    ///
    /// Panics if `len > storage.len()`.
    pub fn with_len(storage: &'a mut [T], len: usize) -> StackVec<'a, T> {
        let cap = storage.len();
        if len > cap {
            panic!("len > storage.len()")
        }
        StackVec { storage: storage, cap: cap, len: len }
    }

    /// Returns the number of elements this vector can hold.
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Shortens the vector, keeping the first `len` elements. If `len` is
    /// greater than the vector's current length, this has no effect. Note that
    /// this method has no effect on the capacity of the vector.
    pub fn truncate(&mut self, len: usize) {
        if len < self.len() {
            self.len = len
        }
    }

    /// Extracts a slice containing the entire vector, consuming `self`.
    ///
    /// Note that the returned slice's length will be the length of this vector,
    /// _not_ the length of the original backing storage.
    pub fn into_slice(self) -> &'a mut [T] {
        let len = self.len();
        &mut self.storage[0..len]
    }

    /// Extracts a slice containing the entire vector.
    pub fn as_slice(&self) -> &[T] {
        &self.storage[0..self.len]
    }

    /// Extracts a mutable slice of the entire vector.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.storage[0..self.len]
    }

    /// Returns the number of elements in the vector, also referred to as its
    /// 'length'.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true if the vector contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns true if the vector is at capacity.
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    /// Appends `value` to the back of this vector if the vector is not full.
    ///
    /// # Error
    ///
    /// If this vector is full, an `Err` is returned. Otherwise, `Ok` is
    /// returned.
    pub fn push(&mut self, value: T) -> Result<(), ()> {
        // stackvec is preallocated; we can run out of space so of course we must return result
        if self.is_full() {
            return Err(());
        }
        self.storage[self.len] = value;
        self.len += 1;
        Ok(())
    }
}

impl<'a, T: Clone + 'a> StackVec<'a, T> {
    /// If this vector is not empty, removes the last element from this vector
    /// by cloning it and returns it. Otherwise returns `None`.
    pub fn pop(&mut self) -> Option<T> {
        // i think we need Clone because the value actually lives in the stack buffer
        // if we didn't clone the result would have lifetime bounded by that of stackvec
        // so therefore, we want to move, but we can't move out of storage as it is borrowed.
        // hence, we need to either copy or clone
        if self.is_empty() {
            return None;
        }
        self.len -= 1;
        Some(self.storage[self.len].clone())
    }
}

impl<'a, T: 'a> Deref for StackVec<'a, T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<'a, T: 'a> DerefMut for StackVec<'a, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<'a, T: 'a> IntoIterator for StackVec<'a, T> {
    type Item = &'a T;
    type IntoIter = slice::Iter<'a, T>;
    fn into_iter(self) -> slice::Iter<'a, T> {
        (&*self.into_slice()).into_iter()
    }
}

impl<'a, T: 'a> IntoIterator for &'a StackVec<'a, T> {
    type Item = &'a T;
    type IntoIter = slice::Iter<'a, T>;
    fn into_iter(self) -> slice::Iter<'a, T> {
        self.as_slice().into_iter()
    }
}

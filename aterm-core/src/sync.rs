//! Fair mutex — prevents render thread starvation.
//! Ported from alacritty_terminal/src/sync.rs.
//!
//! Two internal mutexes: `data` guards the payload, `next` serializes
//! waiters.  The PTY reader thread calls `lease()` to reserve a turn,
//! then `try_lock_unfair()` to avoid blocking.  The render thread
//! calls `lock()`, which respects the queue.

use parking_lot::{Mutex, MutexGuard};

pub struct FairMutex<T> {
    data: Mutex<T>,
    next: Mutex<()>,
}

impl<T> FairMutex<T> {
    pub fn new(data: T) -> FairMutex<T> {
        FairMutex {
            data: Mutex::new(data),
            next: Mutex::new(()),
        }
    }

    /// Reserve the next lock acquisition (PTY reader calls this).
    pub fn lease(&self) -> MutexGuard<'_, ()> {
        self.next.lock()
    }

    /// Fair lock: acquires `next` then `data` (render thread).
    pub fn lock(&self) -> MutexGuard<'_, T> {
        let _next = self.next.lock();
        self.data.lock()
    }

    /// Unfair lock: bypasses `next` queue (PTY reader fallback).
    pub fn lock_unfair(&self) -> MutexGuard<'_, T> {
        self.data.lock()
    }

    /// Non-blocking unfair try-lock (PTY reader primary path).
    pub fn try_lock_unfair(&self) -> Option<MutexGuard<'_, T>> {
        self.data.try_lock()
    }
}

// SAFETY: FairMutex is Send+Sync when T is Send — same invariant as
// parking_lot::Mutex<T> and alacritty's original implementation.
unsafe impl<T: Send> Send for FairMutex<T> {}
unsafe impl<T: Send> Sync for FairMutex<T> {}

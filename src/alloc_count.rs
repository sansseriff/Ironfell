//! Allocations counted per thread, for the scaling test
//! (plans/handoff-scaling.md §5): a clone of a document-sized map shows up
//! here even when nothing iterates it. Test builds only; the counters never
//! allocate, and each thread counts its own, so parallel tests do not mix.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

pub struct Counting;

thread_local! {
    static COUNT: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        let _ = BYTES.try_with(|b| b.set(b.get() + layout.size() as u64));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        let _ = BYTES.try_with(|b| b.set(b.get() + new_size as u64));
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Allocations and bytes allocated on this thread since the last call.
pub fn take() -> (u64, u64) {
    (COUNT.with(|c| c.replace(0)), BYTES.with(|b| b.replace(0)))
}

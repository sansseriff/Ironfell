//! Allocations counted per thread, for the scaling test
//! (plans/handoff-scaling.md §5): a clone of a document-sized map shows up
//! here even when nothing iterates it. Test builds only; the counters never
//! allocate, and each thread counts its own, so parallel tests do not mix.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

pub struct Counting;

thread_local! {
    static COUNT: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
    /// While tracing (`trace`): allocations by the store frames that made
    /// them, for `scaling::trace_one_count` with `WORK_TRACE=allocations`.
    static TRACE: RefCell<Option<HashMap<String, u64>>> = const { RefCell::new(None) };
    static IN_TRACE: Cell<bool> = const { Cell::new(false) };
}

fn note() {
    if IN_TRACE.try_with(Cell::get).unwrap_or(true) {
        return;
    }
    IN_TRACE.with(|i| i.set(true));
    let _ = TRACE.try_with(|t| {
        if let Ok(mut t) = t.try_borrow_mut()
            && let Some(m) = t.as_mut()
        {
            let bt = std::backtrace::Backtrace::force_capture().to_string();
            let frames: Vec<String> = bt
                .lines()
                .filter_map(|l| l.trim().split_once(": ").map(|(_, f)| f.to_owned()))
                .filter(|f| (f.contains("iron_document") || f.contains("ironfell")) && !f.contains("alloc_count") && !f.contains("{closure") && !f.contains("scaling::"))
                .map(|f| f.rsplit_once("::h").map_or(f.clone(), |(a, _)| a.to_owned()))
                .take(5)
                .collect();
            *m.entry(frames.join(" < ")).or_default() += 1;
        }
    });
    IN_TRACE.with(|i| i.set(false));
}

/// Start counting this thread's allocations by stack.
pub fn trace() {
    TRACE.with(|t| *t.borrow_mut() = Some(HashMap::new()));
}

/// The allocations by stack since `trace`, and stop.
pub fn take_trace() -> HashMap<String, u64> {
    TRACE.with(|t| t.borrow_mut().take()).unwrap_or_default()
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        note();
        let _ = BYTES.try_with(|b| b.set(b.get() + layout.size() as u64));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        note();
        let _ = BYTES.try_with(|b| b.set(b.get() + new_size as u64));
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Allocations and bytes allocated on this thread since the last call.
pub fn take() -> (u64, u64) {
    (COUNT.with(|c| c.replace(0)), BYTES.with(|b| b.replace(0)))
}

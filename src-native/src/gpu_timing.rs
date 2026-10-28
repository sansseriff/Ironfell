//! When each frame's GPU work finished (`MTLCommandBuffer.GPUEndTime`, host time),
//! so `late-fifo` can budget for CPU *and* GPU work. wgpu keeps its command buffers
//! private, so the queue's `commandBuffer` / `commandBufferWithUnretainedReferences`
//! are wrapped to attach a completed handler to every buffer it creates. The queue
//! executes buffers in order; a frame's GPU finish is the latest end among the
//! buffers created during it, excluding wgpu's present-only buffer (its last).
use block2::RcBlock;
use objc2::{
    msg_send,
    runtime::{AnyObject, Imp, Sel},
    sel,
};
use std::{
    ptr::NonNull,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
};

#[link(name = "Metal", kind = "framework")]
unsafe extern "C" {
    fn MTLCreateSystemDefaultDevice() -> *mut AnyObject;
}

static PLAIN: OnceLock<Imp> = OnceLock::new();
static UNRETAINED: OnceLock<Imp> = OnceLock::new();
/// Sequence number of the latest command buffer created.
static SEQ: AtomicU64 = AtomicU64::new(0);
/// (sequence, GPUEndTime) of completed buffers.
static COMPLETED: Mutex<Vec<(u64, f64)>> = Mutex::new(Vec::new());

type MakeBuffer = unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> *mut AnyObject;

fn attach(buffer: *mut AnyObject) -> *mut AnyObject {
    if let Some(buffer) = unsafe { buffer.as_ref() } {
        let seq = SEQ.fetch_add(1, Relaxed) + 1;
        let handler = RcBlock::new(move |b: NonNull<AnyObject>| {
            let end: f64 = unsafe { msg_send![b.as_ref(), GPUEndTime] };
            COMPLETED.lock().unwrap().push((seq, end));
        });
        let _: () = unsafe { msg_send![buffer, addCompletedHandler: &*handler] };
    }
    buffer
}

unsafe extern "C-unwind" fn plain(queue: *mut AnyObject, cmd: Sel) -> *mut AnyObject {
    let original: MakeBuffer = unsafe { std::mem::transmute(*PLAIN.get().unwrap()) };
    attach(unsafe { original(queue, cmd) })
}

unsafe extern "C-unwind" fn unretained(queue: *mut AnyObject, cmd: Sel) -> *mut AnyObject {
    let original: MakeBuffer = unsafe { std::mem::transmute(*UNRETAINED.get().unwrap()) };
    attach(unsafe { original(queue, cmd) })
}

/// Wrap the system GPU's command-queue class. Call before the renderer starts.
pub fn install() -> bool {
    static DONE: OnceLock<bool> = OnceLock::new();
    *DONE.get_or_init(|| {
        let Some(device) = (unsafe { MTLCreateSystemDefaultDevice().as_ref() }) else {
            return false;
        };
        let queue: *mut AnyObject = unsafe { msg_send![device, newCommandQueue] };
        let Some(queue) = (unsafe { queue.as_ref() }) else {
            return false;
        };
        let class = queue.class();
        for (selector, slot, ours) in [
            (sel!(commandBuffer), &PLAIN, plain as MakeBuffer),
            (sel!(commandBufferWithUnretainedReferences), &UNRETAINED, unretained as MakeBuffer),
        ] {
            let Some(method) = class.instance_method(selector) else {
                return false;
            };
            let ours: Imp = unsafe { std::mem::transmute(ours) };
            slot.get_or_init(|| unsafe { method.set_implementation(ours) });
        }
        true
    })
}

/// The latest command buffer's sequence number: after a frame, its last buffer.
pub fn last_seq() -> u64 {
    SEQ.load(Relaxed)
}

pub fn take_completed() -> Vec<(u64, f64)> {
    std::mem::take(&mut *COMPLETED.lock().unwrap())
}

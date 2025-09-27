//! seL4 global object allocator and task object allocator.
use alloc::vec::Vec;
use common::ObjectAllocator;
use kspin::SpinNoIrq;
use sel4::{
    Cap,
    cap::{PT, Untyped, Notification},
};

#[percpu::def_percpu]
pub(crate) static OBJ_ALLOCATOR: ObjectAllocator = ObjectAllocator::empty();

pub fn alloc_pt() -> PT {
    unsafe { OBJ_ALLOCATOR.current_ref_raw().alloc_pt() }
}

pub fn alloc_notification() -> Notification {
    unsafe { OBJ_ALLOCATOR.current_ref_raw().alloc_notification() }
}

pub fn init() {
    unsafe { OBJ_ALLOCATOR.current_ref_raw().init(Cap::from_bits(23)); }
}

pub fn alloc_untyped_raw(size_bits: usize) -> Untyped {
    unsafe { OBJ_ALLOCATOR.current_ref_raw().alloc_untyped(size_bits) }
}

const ALLOC_SIZE_BITS: usize = 19; // 512KB

static RECYCLED_UNTYPED: SpinNoIrq<Vec<Untyped>> = SpinNoIrq::new(Vec::new());

pub fn alloc_untyped_unit() -> (Untyped, usize) {
    let cap = match RECYCLED_UNTYPED.lock().pop() {
        Some(cap) => cap,
        None => unsafe {
            OBJ_ALLOCATOR.current_ref_raw().alloc_untyped(ALLOC_SIZE_BITS)
        },
    };
    (cap, 1 << ALLOC_SIZE_BITS)
}

pub fn recycle_untyped_unit(cap: Untyped) {
    RECYCLED_UNTYPED.lock().push(cap);
}

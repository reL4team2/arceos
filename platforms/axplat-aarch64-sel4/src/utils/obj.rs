//! seL4 global object allocator and task object allocator.
use alloc::vec::Vec;
use common::ObjectAllocator;
use kspin::SpinNoIrq;
use sel4::{
    Cap,
    cap::{Granule, PT, Untyped},
};

pub(crate) static OBJ_ALLOCATOR: ObjectAllocator = ObjectAllocator::empty();

pub fn alloc_pt() -> PT {
    OBJ_ALLOCATOR.alloc_pt()
}

pub fn alloc_pages(pn: usize) -> Vec<Granule> {
    OBJ_ALLOCATOR.alloc_pages(pn)
}

pub fn init() {
    OBJ_ALLOCATOR.init(Cap::from_bits(23));
}

const ALLOC_SIZE_BITS: usize = 21; // 2MB

static RECYCLED_UNTYPED: SpinNoIrq<Vec<Untyped>> = SpinNoIrq::new(Vec::new());

pub fn alloc_untyped_unit() -> (Untyped, usize) {
    let cap = match RECYCLED_UNTYPED.lock().pop() {
        Some(cap) => cap,
        None => {
            OBJ_ALLOCATOR.alloc_untyped(ALLOC_SIZE_BITS)
        },
    };
    (cap, 1 << ALLOC_SIZE_BITS)
}

pub fn recycle_untyped_unit(cap: Untyped) {
    RECYCLED_UNTYPED.lock().push(cap);
}

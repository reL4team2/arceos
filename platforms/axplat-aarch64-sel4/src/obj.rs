//! seL4 global object allocator and task object allocator.
use common::ObjectAllocator;
use kspin::SpinRaw;
use sel4::{Cap, CapRights, cap::Untyped};
use sel4_kit::slot_manager::LeafSlot;

use lazyinit::LazyInit;
use sel4_oskit::allocator::UntypedAllocator;

static UNTYPED_ALLOCATOR: LazyInit<SpinRaw<UntypedAllocator>> = LazyInit::new();
pub const ALLOC_SIZE_BITS: usize = 18; // 256KB

#[percpu::def_percpu]
pub(crate) static OBJ_ALLOCATOR: ObjectAllocator = ObjectAllocator::empty();

pub(crate) fn init() {
    unsafe {
        OBJ_ALLOCATOR.current_ref_raw().init(Cap::from_bits(23));
    }

    sel4::init_thread::slot::CNODE
        .cap()
        .absolute_cptr_from_bits_with_depth(0x90 as _, 64)
        .copy(
            &LeafSlot::from_cap(sel4::init_thread::slot::CNODE.cap()).abs_cptr(),
            CapRights::all(),
        )
        .unwrap();

    UNTYPED_ALLOCATOR.init_once(SpinRaw::new(UntypedAllocator::new(
        unsafe { OBJ_ALLOCATOR.current_ref_raw() },
        ALLOC_SIZE_BITS,
        axconfig::plat::CPU_NUM,
    )));
}

pub(crate) fn init_secondary() {
    unsafe {
        OBJ_ALLOCATOR.current_ref_raw().init(Cap::from_bits(23));
    }

    for i in 1..axconfig::plat::CPU_NUM {
        let _ = sel4::init_thread::slot::CNODE
            .cap()
            .absolute_cptr_from_bits_with_depth((0x90 + i) as _, 64)
            .copy(
                &LeafSlot::new(0x90)
                    .cap()
                    .absolute_cptr_from_bits_with_depth((0x90 + i) as _, 64),
                CapRights::all(),
            );
    }
}

pub(crate) fn alloc_untyped_raw(size: usize) -> Untyped {
    unsafe { OBJ_ALLOCATOR.current_ref_raw().alloc_untyped(size) }
}

pub(crate) fn alloc_untyped(cpu_id: usize) -> Untyped {
    UNTYPED_ALLOCATOR.lock().alloc(cpu_id)
}

pub(crate) fn recycle_untyped(cap: Untyped, cpu_id: usize) {
    UNTYPED_ALLOCATOR.lock().dealloc(cap, cpu_id);
}

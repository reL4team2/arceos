//! seL4 global object allocator and task object allocator.
use alloc::vec::Vec;
use common::ObjectAllocator;
use kspin::SpinNoIrq;
use sel4::{
    AbsoluteCPtr, Cap, CapRights,
    cap::{CNode, Notification, PT, Untyped},
};
use sel4_kit::slot_manager::LeafSlot;

#[percpu::def_percpu]
pub(crate) static OBJ_ALLOCATOR: ObjectAllocator = ObjectAllocator::empty();

pub fn alloc_pt() -> PT {
    unsafe { OBJ_ALLOCATOR.current_ref_raw().alloc_pt() }
}

pub fn alloc_notification() -> Notification {
    unsafe { OBJ_ALLOCATOR.current_ref_raw().alloc_notification() }
}

pub fn init() {
    unsafe {
        OBJ_ALLOCATOR.current_ref_raw().init(Cap::from_bits(23));
    }
}

pub fn alloc_untyped_raw(size_bits: usize) -> Untyped {
    unsafe { OBJ_ALLOCATOR.current_ref_raw().alloc_untyped(size_bits) }
}

pub(crate) struct IndexAllocator {
    next: usize,
    max: usize,
    recycled: Vec<u64>,
}

impl IndexAllocator {
    pub const fn new(next: usize, max: usize) -> Self {
        assert!(next <= max);
        Self {
            next,
            max,
            recycled: Vec::new(),
        }
    }

    pub fn alloc(&mut self) -> Option<usize> {
        if let Some(index) = self.recycled.pop() {
            Some(index as usize)
        } else if self.next < self.max {
            let index = self.next;
            self.next += 1;
            Some(index)
        } else {
            None
        }
    }

    pub fn recycle(&mut self, index: usize) {
        self.recycled.push(index as u64);
    }
}

pub(crate) struct TaskCapSet {
    index_allocator: IndexAllocator,
    cnode: CNode,
    cnode_bits: usize,
    cnode_index: usize,
}

impl TaskCapSet {
    pub fn new(cnode: CNode, cnode_bits: usize, start_index: usize, cnode_index: usize) -> Self {
        Self {
            index_allocator: IndexAllocator::new(start_index, 1 << cnode_bits),
            cnode,
            cnode_bits,
            cnode_index,
        }
    }

    pub fn add_cap<T: sel4::CapType>(
        &mut self,
        index: Option<usize>,
        src: &AbsoluteCPtr,
    ) -> sel4::Result<Cap<T>> {
        let idx = match index {
            Some(index) => index,
            None => self.index_allocator.alloc().unwrap(),
        };

        self.cnode
            .absolute_cptr_from_bits_with_depth(idx as _, self.cnode_bits)
            .move_(src)?;

        let cap = LeafSlot::new((self.cnode_index << self.cnode_bits) + idx as usize).cap();
        Ok(cap)
    }
}

const ALLOC_SIZE_BITS: usize = 19; // 512KB

static RECYCLED_UNTYPED: SpinNoIrq<Vec<Untyped>> = SpinNoIrq::new(Vec::new());

pub fn alloc_untyped_unit() -> (Untyped, usize) {
    let cap = match RECYCLED_UNTYPED.lock().pop() {
        Some(cap) => cap,
        None => unsafe {
            OBJ_ALLOCATOR
                .current_ref_raw()
                .alloc_untyped(ALLOC_SIZE_BITS)
        },
    };
    (cap, 1 << ALLOC_SIZE_BITS)
}

pub fn recycle_untyped_unit(cap: Untyped) {
    RECYCLED_UNTYPED.lock().push(cap);
}

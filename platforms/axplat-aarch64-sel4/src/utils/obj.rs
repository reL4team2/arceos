//! seL4 global object allocator and task object allocator.
use alloc::vec::Vec;
use common::{ObjectAllocator, slot::{alloc_slot, recycle_slot}};
use kspin::SpinNoIrq;
use sel4::{
    AbsoluteCPtr, Cap, cap_type, CapRights, CNodeCapData,
    cap::{CNode, Notification, PT, Untyped},
    CapTypeForObjectOfFixedSize, CapTypeForObjectOfVariableSize,
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

pub fn alloc_cnode(size_bits: usize) -> CNode {
    unsafe { OBJ_ALLOCATOR.current_ref_raw().alloc_cnode(size_bits) }
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

pub struct CapSet {
    root_cnode: AbsoluteCPtr,
    root_cnode_bits: usize,
    index_allocator: IndexAllocator,
    untyped: (Untyped, usize),
    caps: Vec<usize>,
}

impl CapSet {
    pub fn new(
        cnode_index: usize,
        root_cnode_bits: usize,
        untyped_cap: Untyped,
        untyped_size: usize,
        start_index: usize,
    ) -> sel4::Result<Self> {
        // alloc cnode from untyped cap
        let cnode_size = 1 << cap_type::CNode::object_blueprint(root_cnode_bits).physical_size_bits();
        assert!(untyped_size >= cnode_size);

        // TODO: create global slot manager for each CPU core
        let cnode_slot = alloc_slot();
        untyped_cap.untyped_retype(&cap_type::CNode::object_blueprint(root_cnode_bits), &cnode_slot.cnode_abs_cptr(), cnode_slot.offset_of_cnode(), 1)?;
        let untyped_size = untyped_size - cnode_size;

        let cnode = cnode_slot.cap();

        cnode.absolute_cptr_from_bits_with_depth(2, root_cnode_bits).mint(
            &LeafSlot::from_cap(cnode).abs_cptr(),
            CapRights::all(),
            CNodeCapData::skip_high_bits(root_cnode_bits).into_word() as _,
        )?;

        // move cnode to parent cspace
        sel4::init_thread::slot::CNODE
            .cap()
            .absolute_cptr_from_bits_with_depth(cnode_index as _, 64 - root_cnode_bits)
            .move_(&LeafSlot::from_cap(cnode).abs_cptr())?;

        let root_cnode = sel4::init_thread::slot::CNODE
            .cap()
            .absolute_cptr_from_bits_with_depth(cnode_index as _, 64 - root_cnode_bits);

        recycle_slot(cnode_slot);

        Ok(Self {
            root_cnode,
            root_cnode_bits,
            index_allocator: IndexAllocator::new(start_index, 1 << root_cnode_bits),
            untyped: (untyped_cap, untyped_size),
            caps: Vec::new(),
        })
    }

    pub fn check_available(&self, size: usize) -> sel4::Result<()> {
        if self.untyped.1 < size {
            return Err(sel4::Error::NotEnoughMemory);
        }

        Ok(())
    }

    pub fn alloc_fixed<T: CapTypeForObjectOfFixedSize>(&mut self, idx: Option<usize>) -> sel4::Result<LeafSlot> {
        let phys_size = 1 << T::object_blueprint().physical_size_bits();
        self.check_available(phys_size)?;
        
        // Allocate a slot in the CNode
        let index = match idx {
            Some(i) => i,
            None => self.index_allocator.alloc().ok_or(sel4::Error::NotEnoughMemory)?,
        };

        // Allocate the object from the untyped capability
        self.untyped.0.untyped_retype(&T::object_blueprint(), &self.root_cnode, index as _, 1)?;
        self.untyped.1 -= phys_size;

        self.caps.push(index);

        let slot = LeafSlot::new(((self.root_cnode.path().bits() as usize) << self.root_cnode_bits) + index);
        Ok(slot)
    }

    pub fn alloc_variable<T: CapTypeForObjectOfVariableSize>(&mut self, idx: Option<usize>, size_bits: usize) -> sel4::Result<LeafSlot> {
        let phys_size = 1 << T::object_blueprint(size_bits).physical_size_bits();
        self.check_available(phys_size)?;

        // Allocate a slot in the CNode
        let index = match idx {
            Some(i) => i,
            None => self.index_allocator.alloc().ok_or(sel4::Error::NotEnoughMemory)?,
        };

        // Allocate the object from the untyped capability
        self.untyped.0.untyped_retype(&T::object_blueprint(size_bits), &self.root_cnode, index as _, 1)?;
        self.untyped.1 -= phys_size;

        self.caps.push(index);

        let slot = LeafSlot::new(((self.root_cnode.path().bits() as usize) << self.root_cnode_bits) + index);
        Ok(slot)
    }

    pub fn alloc_page(&mut self, idx: Option<usize>) -> sel4::Result<Cap<cap_type::Granule>> {
        Ok(self.alloc_fixed::<cap_type::Granule>(idx)?.into())
    }

    pub fn alloc_pt(&mut self, idx: Option<usize>) -> sel4::Result<Cap<cap_type::PT>> {
        Ok(self.alloc_fixed::<cap_type::PT>(idx)?.into())
    }

    pub fn alloc_cnode(&mut self, idx: Option<usize>, size_bits: usize) -> sel4::Result<Cap<cap_type::CNode>> {
        Ok(self.alloc_variable::<cap_type::CNode>(idx, size_bits)?.into())
    }

    pub fn alloc_tcb(&mut self, idx: Option<usize>) -> sel4::Result<Cap<cap_type::Tcb>> {
        Ok(self.alloc_fixed::<cap_type::Tcb>(idx)?.into())
    }

    pub fn alloc_notification(&mut self, idx: Option<usize>) -> sel4::Result<Cap<cap_type::Notification>> {
        Ok(self.alloc_fixed::<cap_type::Notification>(idx)?.into())
    }

    pub fn alloc_endpoint(&mut self, idx: Option<usize>) -> sel4::Result<Cap<cap_type::Endpoint>> {
        Ok(self.alloc_fixed::<cap_type::Endpoint>(idx)?.into())
    }

    pub fn root_cnode(&self) -> Cap<cap_type::CNode> {
        LeafSlot::new(((self.root_cnode.path().bits() as usize) << self.root_cnode_bits) + 2).cap()
    }

    pub fn exit(&self) {
        // delete all allocated caps
        for idx in &self.caps {
            let abs_path = self.root_cnode().absolute_cptr_from_bits_with_depth(*idx as _, 64);
            abs_path.revoke().unwrap();
            abs_path.delete().unwrap();
        }

        recycle_untyped_unit(self.untyped.0);
    }

    pub fn migrate(&mut self, root_cnode: AbsoluteCPtr) {
        assert_eq!(self.root_cnode_bits, 64 - root_cnode.path().depth() as usize);
        self.root_cnode = root_cnode;
    }
}

pub const ALLOC_SIZE_BITS: usize = 19; // 512KB

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

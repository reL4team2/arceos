pub mod allocator;

use alloc::vec::Vec;
use kspin::SpinNoIrq;

use common::slot::{alloc_slot, recycle_slot};

use sel4::{
    AbsoluteCPtr, CNodeCapData, Cap, CapRights, CapTypeForObjectOfFixedSize,
    CapTypeForObjectOfVariableSize, cap::Untyped, cap_type,
};

use sel4_kit::slot_manager::{LeafSlot, SlotManager};

use crate::config::*;

pub(crate) trait MemCapAllocator {
    fn alloc_pt(&self) -> sel4::Result<Cap<cap_type::PT>>;
    fn alloc_page(&self) -> sel4::Result<Cap<cap_type::Granule>>;
    fn alloc_pages(&self, count: usize) -> sel4::Result<Vec<Cap<cap_type::Granule>>>;
    fn alloc_large_page(&self) -> sel4::Result<Cap<cap_type::LargePage>>;
    fn alloc_large_pages(&self, count: usize) -> sel4::Result<Vec<Cap<cap_type::LargePage>>>;
}

pub struct CapSet {
    root_cnode: AbsoluteCPtr,
    root_cnode_bits: usize,
    slot_manager: SpinNoIrq<SlotManager>,
    untyped: Untyped,
    caps: SpinNoIrq<Vec<usize>>,
}

impl CapSet {
    pub fn new(
        cnode_index: usize,
        root_cnode_bits: usize,
        untyped_cap: Untyped,
        start_index: usize,
    ) -> sel4::Result<Self> {
        // alloc cnode from untyped cap
        // TODO: create global slot manager for each CPU core
        let cnode_slot = alloc_slot();
        untyped_cap.untyped_retype(
            &cap_type::CNode::object_blueprint(root_cnode_bits),
            &cnode_slot.cnode_abs_cptr(),
            cnode_slot.offset_of_cnode(),
            1,
        )?;

        let cnode = cnode_slot.cap();
        cnode
            .absolute_cptr_from_bits_with_depth(2, root_cnode_bits)
            .mint(
                &LeafSlot::from_cap(cnode).abs_cptr(),
                CapRights::all(),
                CNodeCapData::skip_high_bits(root_cnode_bits).into_word() as _,
            )?;

        // move cnode to parent cspace
        let _ = sel4::init_thread::slot::CNODE
            .cap()
            .absolute_cptr_from_bits_with_depth(cnode_index as _, CSPACE_DEPTH - root_cnode_bits)
            .delete();

        sel4::init_thread::slot::CNODE
            .cap()
            .absolute_cptr_from_bits_with_depth(cnode_index as _, CSPACE_DEPTH - root_cnode_bits)
            .move_(&LeafSlot::from_cap(cnode).abs_cptr())?;

        let root_cnode = sel4::init_thread::slot::CNODE
            .cap()
            .absolute_cptr_from_bits_with_depth(cnode_index as _, CSPACE_DEPTH - root_cnode_bits);

        recycle_slot(cnode_slot);

        let slot_start_index = (cnode_index << root_cnode_bits) + start_index;
        let slot_end_index = (cnode_index + 1) << root_cnode_bits;

        Ok(Self {
            root_cnode,
            root_cnode_bits,
            slot_manager: SpinNoIrq::new(SlotManager::new(slot_start_index..slot_end_index)),
            untyped: untyped_cap,
            caps: SpinNoIrq::new(Vec::new()),
        })
    }

    pub fn alloc_fixed<T: CapTypeForObjectOfFixedSize>(
        &self,
        idx: Option<usize>,
    ) -> sel4::Result<LeafSlot> {
        // Allocate a slot in the CNode
        let index = match idx {
            Some(i) => i,
            None => self.slot_manager.lock().alloc_slot().offset_of_cnode(),
        };

        // Allocate the object from the untyped capability
        self.untyped
            .untyped_retype(&T::object_blueprint(), &self.root_cnode, index as _, 1)?;

        self.caps.lock().push(index);

        let slot = LeafSlot::new(
            ((self.root_cnode.path().bits() as usize) << self.root_cnode_bits) + index,
        );
        Ok(slot)
    }

    pub fn alloc_variable<T: CapTypeForObjectOfVariableSize>(
        &self,
        idx: Option<usize>,
        size_bits: usize,
    ) -> sel4::Result<LeafSlot> {
        // Allocate a slot in the CNode
        let index = match idx {
            Some(i) => i,
            None => self.slot_manager.lock().alloc_slot().offset_of_cnode(),
        };

        // Allocate the object from the untyped capability
        self.untyped.untyped_retype(
            &T::object_blueprint(size_bits),
            &self.root_cnode,
            index as _,
            1,
        )?;

        self.caps.lock().push(index);

        let slot = LeafSlot::new(
            ((self.root_cnode.path().bits() as usize) << self.root_cnode_bits) + index,
        );
        Ok(slot)
    }

    pub fn alloc_cnode(
        &mut self,
        idx: Option<usize>,
        size_bits: usize,
    ) -> sel4::Result<Cap<cap_type::CNode>> {
        Ok(self
            .alloc_variable::<cap_type::CNode>(idx, size_bits)?
            .into())
    }

    pub fn alloc_tcb(&mut self, idx: Option<usize>) -> sel4::Result<Cap<cap_type::Tcb>> {
        Ok(self.alloc_fixed::<cap_type::Tcb>(idx)?.into())
    }

    pub fn alloc_notification(
        &mut self,
        idx: Option<usize>,
    ) -> sel4::Result<Cap<cap_type::Notification>> {
        Ok(self.alloc_fixed::<cap_type::Notification>(idx)?.into())
    }

    pub fn alloc_endpoint(&mut self, idx: Option<usize>) -> sel4::Result<Cap<cap_type::Endpoint>> {
        Ok(self.alloc_fixed::<cap_type::Endpoint>(idx)?.into())
    }

    pub fn root_cnode(&self) -> Cap<cap_type::CNode> {
        LeafSlot::new(((self.root_cnode.path().bits() as usize) << self.root_cnode_bits) + 2).cap()
    }

    pub fn root_cnode_path(&self) -> AbsoluteCPtr {
        self.root_cnode
    }

    pub fn drop(&self) -> sel4::Result<()> {
        // delete all allocated caps
        for idx in self.caps.lock().iter() {
            let abs_path = self
                .root_cnode()
                .absolute_cptr_from_bits_with_depth(*idx as _, CSPACE_DEPTH);
            abs_path.revoke()?;
            abs_path.delete()?;
        }

        Ok(())
    }

    pub fn migrate(&mut self, root_cnode: AbsoluteCPtr) {
        assert_eq!(
            self.root_cnode_bits,
            CSPACE_DEPTH - root_cnode.path().depth() as usize
        );
        self.root_cnode = root_cnode;
    }
}

impl MemCapAllocator for CapSet {
    fn alloc_pt(&self) -> sel4::Result<Cap<cap_type::PT>> {
        Ok(self.alloc_fixed::<cap_type::PT>(None)?.into())
    }

    fn alloc_page(&self) -> sel4::Result<Cap<cap_type::Granule>> {
        Ok(self.alloc_fixed::<cap_type::Granule>(None)?.into())
    }

    fn alloc_large_page(&self) -> sel4::Result<Cap<cap_type::LargePage>> {
        Ok(self.alloc_fixed::<cap_type::LargePage>(None)?.into())
    }

    fn alloc_pages(&self, count: usize) -> sel4::Result<Vec<Cap<cap_type::Granule>>> {
        let mut pages = Vec::new();
        for _ in 0..count {
            pages.push(self.alloc_page()?);
        }
        Ok(pages)
    }

    fn alloc_large_pages(&self, count: usize) -> sel4::Result<Vec<Cap<cap_type::LargePage>>> {
        let mut pages = Vec::new();
        for _ in 0..count {
            pages.push(self.alloc_large_page()?);
        }
        Ok(pages)
    }
}

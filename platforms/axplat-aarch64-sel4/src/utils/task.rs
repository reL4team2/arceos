//! This module provides the [Sel4Task] struct, which represents a task in the seL4 environment.
//! It provides methods for initializing and managing the task's resources.
//! It helps us create seL4 tasks in arceos, when porting arceos on seL4.
//! The [Sel4Task] struct encapsulates the task's TCB, CNode, entry point, stack, and capability set.

use common::{
    ObjectAllocator,
    config::{CNODE_RADIX_BITS, DEFAULT_PARENT_EP, DEFAULT_SERVE_EP},
    slot::recycle_slot,
};

use sel4::{
    CNodeCapData, CapRights,
    cap::{self, CNode, Endpoint, Granule, Tcb, Untyped},
    init_thread,
};
use sel4_kit::slot_manager::LeafSlot;

use alloc::sync::Arc;

use super::obj::{alloc_untyped_raw, alloc_untyped_unit, recycle_untyped_unit};
use crate::mem::{alloc_ipc_buffer, dealloc_ipc_buffer};

unsafe extern "C" {
    fn _stdata();
    fn _etdata();
    fn _etbss();
}

/// Basic unit representing a task in seL4.
pub struct Sel4Task {
    pub tcb: cap::Tcb,
    pub cnode: cap::CNode,
    pub ep: cap::Endpoint,
    pub entry: usize,
    pub stack: usize,
    pub capset: ObjectAllocator,
    pub untyped: cap::Untyped,
    pub ipc_buffer: cap::Granule,
    pub ipc_buffer_addr: usize,
    pub tid: usize,
    pub affinity: usize,
}

impl Sel4Task {
    /// Create a empty Sel4Task Struct
    pub fn empty() -> Sel4Task {
        Self {
            tcb: Tcb::from_bits(0),
            cnode: CNode::from_bits(0),
            ep: Endpoint::from_bits(0),
            entry: 0,
            stack: 0,
            capset: ObjectAllocator::empty(),
            untyped: Untyped::from_bits(0),
            ipc_buffer: Granule::from_bits(0),
            ipc_buffer_addr: 0,
            tid: 0,
            affinity: 0,
        }
    }

    /// Initialize a new Sel4Task with the given parameters.
    /// This method allocates a TCB, a CNode, and an IPC buffer,
    /// and configures the TCB with the provided entry point and stack.
    pub fn new(
        tid: usize,
        entry: usize,
        stack: usize,
        priority: usize,
        _tls: usize,
        affinity: usize,
    ) -> sel4::Result<Self> {
        log::info!(
            "create new task: tid: {:#x}, entry: {:#x}, stack: {:#x}",
            tid,
            entry,
            stack
        );

        let (untyped, _) = alloc_untyped_unit();
        let obj_allocator = ObjectAllocator::empty();
        obj_allocator.init(untyped);

        // create a 1-level cspace
        let cnode = obj_allocator.alloc_cnode(CNODE_RADIX_BITS);

        // create a new tcb
        let tcb = obj_allocator.alloc_tcb();

        // create a endpoint for task
        let srv_ep = obj_allocator.alloc_endpoint();

        // copy tcb into thread cspace
        cnode
            .absolute_cptr_from_bits_with_depth(1, CNODE_RADIX_BITS)
            .copy(&LeafSlot::from_cap(tcb).abs_cptr(), CapRights::all())?;

        // copy parent endpoint to child
        cnode
            .absolute_cptr_from_bits_with_depth(DEFAULT_PARENT_EP.bits(), CNODE_RADIX_BITS)
            .mint(
                &LeafSlot::from(DEFAULT_SERVE_EP).abs_cptr(),
                CapRights::all(),
                tid as _,
            )?;

        // copy srv endpoint to cnode
        cnode
            .absolute_cptr_from_bits_with_depth(DEFAULT_SERVE_EP.bits(), CNODE_RADIX_BITS)
            .copy(&LeafSlot::from_cap(srv_ep).abs_cptr(), CapRights::all())?;

        log::info!("lxy debug1");
        let (virt, ipc_cap) = alloc_ipc_buffer(&obj_allocator).unwrap();
        log::info!("lxy debug2");

        // configure thread tcb
        tcb.tcb_configure(
            DEFAULT_PARENT_EP.cptr(),
            cnode,
            CNodeCapData::skip_high_bits(CNODE_RADIX_BITS),
            sel4::init_thread::slot::VSPACE.cap(),
            virt as _,
            ipc_cap,
        )
        .unwrap();

        let mut sp = stack;
        if _tls > 0 {
            tcb.tcb_set_tls_base(_tls as _).unwrap();
        } else {
            // reserve tls region on stack
            sp = stack - 0x100;
            tcb.tcb_set_tls_base(stack as _).unwrap();
        }

        tcb.tcb_set_sched_params(sel4::init_thread::slot::TCB.cap(), 0, priority as _)
            .unwrap();

        // set init context
        let mut regs = tcb.tcb_read_all_registers(true).unwrap();
        *regs.pc_mut() = entry as _;
        *regs.sp_mut() = sp as _;
        *regs.gpr_mut(8) = virt as _;
        *regs.gpr_mut(0) = affinity as _;
        unsafe {
            core::arch::asm!(
                "str x28, [{0}]",
                in(reg) regs.gpr_mut(28),
                options(nostack, preserves_flags)
            );
        }

        tcb.tcb_write_all_registers(false, &mut regs).unwrap();

        // set affinity
        tcb.tcb_set_affinity(affinity as _).unwrap();

        let task = Self {
            tcb,
            cnode,
            ep: srv_ep,
            entry,
            stack: sp,
            capset: obj_allocator,
            untyped,
            ipc_buffer: ipc_cap,
            ipc_buffer_addr: virt,
            tid,
            affinity,
        };

        Ok(task)
    }

    pub fn new_init_task(entry: usize, stack: usize, affinity: usize) -> sel4::Result<Self> {
        let (untyped, _) = alloc_untyped_unit();
        let obj_allocator = ObjectAllocator::empty();
        obj_allocator.init(untyped);
        let tid = 0xF000 + affinity;

        // create a 2-level cspace
        log::info!("create 2-level cspace for init task");
        let cnode = obj_allocator.alloc_cnode(CNODE_RADIX_BITS);
        log::info!("first level cnode: {:#x}", cnode.bits());
        let inner_cnode = obj_allocator.alloc_cnode(CNODE_RADIX_BITS);
        log::info!("second level cnode: {:#x}", inner_cnode.bits());
        cnode
            .absolute_cptr_from_bits_with_depth(0, CNODE_RADIX_BITS)
            .mutate(
                &LeafSlot::from_cap(inner_cnode).abs_cptr(),
                CNodeCapData::skip(0).into_word() as _,
            )
            .unwrap();
        LeafSlot::new(0)
            .abs_cptr()
            .mutate(
                &LeafSlot::new(cnode.bits() as _).abs_cptr(),
                CNodeCapData::skip_high_bits(2 * CNODE_RADIX_BITS).into_word() as _,
            )
            .unwrap();
        LeafSlot::new(cnode.bits() as _)
            .abs_cptr()
            .mutate(
                &LeafSlot::new(0).abs_cptr(),
                CNodeCapData::skip_high_bits(2 * CNODE_RADIX_BITS).into_word() as _,
            )
            .unwrap();

        // create a new tcb
        let tcb = obj_allocator.alloc_tcb();
        log::info!("create tcb for init task: {:#x}", tcb.bits());
        // create a endpoint for task
        let ep = obj_allocator.alloc_endpoint();
        log::info!("create ep for init task: {:#x}", ep.bits());

        cnode.absolute_cptr(init_thread::slot::CNODE.cptr()).mint(
            &LeafSlot::from_cap(cnode).abs_cptr(),
            CapRights::all(),
            CNodeCapData::skip_high_bits(2 * CNODE_RADIX_BITS).into_word(),
        )?;

        // copy tcb into thread cspace
        cnode
            .absolute_cptr(init_thread::slot::TCB.cptr())
            .copy(&LeafSlot::from_cap(tcb).abs_cptr(), CapRights::all())?;
        log::info!("init task tcb: {:#x}", tcb.bits());
        // copy parent endpoint to child
        cnode.absolute_cptr(DEFAULT_PARENT_EP.cptr()).mint(
            &LeafSlot::from(DEFAULT_SERVE_EP).abs_cptr(),
            CapRights::all(),
            tid as _,
        )?;

        log::info!("init task parent ep: {:#x}", DEFAULT_PARENT_EP.bits());
        // copy srv endpoint to cnode
        cnode
            .absolute_cptr(DEFAULT_SERVE_EP.cptr())
            .copy(&LeafSlot::from_cap(ep).abs_cptr(), CapRights::all())?;

        let (virt, ipc_cap) = alloc_ipc_buffer(&obj_allocator).unwrap();
        log::info!("init task ipc buffer: {:#x}", ipc_cap.bits());
        // copy untyped into cnode
        let untyped_raw = alloc_untyped_raw(22);

        cnode.absolute_cptr_from_bits_with_depth(23, 64).copy(
            &LeafSlot::from_cap(untyped_raw).abs_cptr(),
            CapRights::all(),
        )?;

        log::info!("init task untyped: {:#x}", untyped_raw.bits());

        tcb.tcb_configure(
            DEFAULT_PARENT_EP.cptr(),
            cnode,
            CNodeCapData::skip_high_bits(2 * CNODE_RADIX_BITS),
            sel4::init_thread::slot::VSPACE.cap(),
            virt as _,
            ipc_cap,
        )?;

        // reserve tls region on stack
        let sp = stack - 0x100;
        tcb.tcb_set_tls_base(stack as _)?;

        tcb.tcb_set_sched_params(sel4::init_thread::slot::TCB.cap(), 255, 255)?;

        let mut regs = tcb.tcb_read_all_registers(true)?;
        *regs.pc_mut() = entry as _;
        *regs.sp_mut() = sp as _;
        *regs.gpr_mut(8) = virt as _;
        *regs.gpr_mut(0) = affinity as _;

        tcb.tcb_write_all_registers(false, &mut regs)?;

        tcb.tcb_set_affinity(affinity as _)?;

        Ok(Self {
            tcb,
            cnode,
            ep,
            entry,
            stack: sp,
            capset: obj_allocator,
            untyped,
            ipc_buffer: ipc_cap,
            ipc_buffer_addr: virt,
            tid,
            affinity,
        })
    }

    pub fn start(&self) -> sel4::Result<()> {
        self.tcb.tcb_resume()
    }

    pub fn set_affinity(&self, affinity: usize) -> sel4::Result<()> {
        self.tcb.tcb_set_affinity(affinity as _)
    }

    pub fn suspend(&self) -> sel4::Result<()> {
        self.tcb.tcb_suspend()
    }

    pub fn exit(&self) {
        let root_cnode = sel4::init_thread::slot::CNODE.cap();
        root_cnode.absolute_cptr(self.tcb).revoke().unwrap();
        root_cnode.absolute_cptr(self.tcb).delete().unwrap();
        root_cnode.absolute_cptr(self.cnode).revoke().unwrap();
        root_cnode.absolute_cptr(self.cnode).delete().unwrap();
        root_cnode.absolute_cptr(self.ep).revoke().unwrap();
        root_cnode.absolute_cptr(self.ep).delete().unwrap();
        root_cnode.absolute_cptr(self.ipc_buffer).revoke().unwrap();
        root_cnode.absolute_cptr(self.ipc_buffer).delete().unwrap();
        recycle_slot(self.tcb.into());
        recycle_slot(self.cnode.into());
        recycle_slot(self.ep.into());
        recycle_slot(self.ipc_buffer.into());
        dealloc_ipc_buffer(self.ipc_buffer_addr);
        recycle_untyped_unit(self.untyped);
    }
}

pub fn create_sel4_task(
    tid: usize,
    entry: usize,
    stack: usize,
    tls: usize,
    affinity: usize,
) -> usize {
    let t = Arc::new(Sel4Task::new(tid, entry, stack, 100, tls, affinity).unwrap());
    let ptr = Arc::into_raw(t);
    ptr as usize
}

pub fn exit_sel4_task(task_ptr: usize) {
    let t = unsafe { Arc::from_raw(task_ptr as *const Sel4Task) };
    log::debug!("exit sel4 task, tid: {}", t.tid);
    t.exit();
}

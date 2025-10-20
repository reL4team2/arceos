//! This module provides the [Sel4Task] struct, which represents a task in the seL4 environment.
//! It provides methods for initializing and managing the task's resources.
//! It helps us create seL4 tasks in arceos, when porting arceos on seL4.
//! The [Sel4Task] struct encapsulates the task's TCB, CNode, entry point, stack, and capability set.

use common::{
    ObjectAllocator,
    config::{CNODE_RADIX_BITS, DEFAULT_PARENT_EP, DEFAULT_SERVE_EP},
};

use sel4::{
    CNodeCapData, CapRights,
    cap::{self},
    init_thread,
};
use sel4_kit::slot_manager::LeafSlot;

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use kspin::SpinNoIrq;

use super::obj::{ALLOC_SIZE_BITS, CapSet, IndexAllocator, alloc_untyped_raw, alloc_untyped_unit};
use crate::mem::{alloc_ipc_buffer, alloc_ipc_buffer_by_capset, dealloc_ipc_buffer};

unsafe extern "C" {
    fn _stdata();
    fn _etdata();
    fn _etbss();
}

static TASK_CSPACE_ALLOCATOR: SpinNoIrq<IndexAllocator> =
    const { SpinNoIrq::new(IndexAllocator::new(1, 4096 - 1)) };

static TASK_MAP: SpinNoIrq<BTreeMap<usize, Arc<SpinNoIrq<NormalTask>>>> =
    SpinNoIrq::new(BTreeMap::new());

/// Basic unit representing a task in seL4.
pub struct NormalTask {
    pub entry: usize,
    pub stack: usize,
    pub capset: CapSet,
    pub untyped: cap::Untyped,
    pub ipc_buffer_addr: usize,
    pub tid: usize,
    pub affinity: usize,
    pub cnode_index: usize,
    pub migrate: bool,
}

impl NormalTask {
    /// Initialize a new NormalTask with the given parameters.
    /// This method allocates a TCB, a CNode, and an IPC buffer,
    /// and configures the TCB with the provided entry point and stack.
    pub fn new(
        tid: usize,
        entry: usize,
        stack: usize,
        priority: usize,
        _tls: usize,
        cpu_id: usize,
    ) -> sel4::Result<Self> {
        // allocate untyped for task
        let (untyped, _) = alloc_untyped_unit(cpu_id);
        let cnode_index = TASK_CSPACE_ALLOCATOR
            .lock()
            .alloc()
            .expect("no more cnode index");

        let mut capset = CapSet::new(
            cnode_index,
            CNODE_RADIX_BITS,
            untyped,
            1 << ALLOC_SIZE_BITS,
            0x100,
            cpu_id,
        )
        .unwrap();

        // create a new tcb
        let tcb = capset.alloc_tcb(Some(1))?;

        // create a endpoint for task
        capset
            .alloc_endpoint(Some(DEFAULT_SERVE_EP.bits() as usize))
            .unwrap();

        // copy parent endpoint to child
        capset
            .root_cnode()
            .absolute_cptr_from_bits_with_depth(DEFAULT_PARENT_EP.bits(), 64)
            .mint(
                &LeafSlot::from(DEFAULT_SERVE_EP).abs_cptr(),
                CapRights::all(),
                tid as _,
            )
            .unwrap();

        let (virt, ipc_cap) = alloc_ipc_buffer_by_capset(&mut capset)?;

        for i in 0..axconfig::plat::CPU_NUM {
            if i == cpu_id {
                continue;
            }

            let _ = LeafSlot::new(0x90 + i)
                .cap()
                .absolute_cptr_from_bits_with_depth(cnode_index as _, 52)
                .delete();

            let _ = LeafSlot::new(0x90 + i)
                .cap()
                .absolute_cptr_from_bits_with_depth(cnode_index as _, 52)
                .copy(&capset.root_cnode_path(), CapRights::all());
        }

        // configure thread tcb
        tcb.tcb_configure(
            DEFAULT_PARENT_EP.cptr(),
            capset.root_cnode(),
            CNodeCapData::skip_high_bits(CNODE_RADIX_BITS),
            init_thread::slot::VSPACE.cap(),
            virt as _,
            ipc_cap,
        )
        .unwrap();

        let mut sp = stack;
        if _tls > 0 {
            tcb.tcb_set_tls_base(_tls as _)?;
        } else {
            // reserve tls region on stack
            sp = stack - 0x100;
            tcb.tcb_set_tls_base(stack as _)?;
        }

        tcb.tcb_set_sched_params(sel4::init_thread::slot::TCB.cap(), 0, priority as _)?;

        // set init context
        let mut regs = tcb.tcb_read_all_registers(true)?;
        *regs.pc_mut() = entry as _;
        *regs.sp_mut() = sp as _;
        *regs.gpr_mut(8) = virt as _;
        *regs.gpr_mut(0) = cpu_id as _;
        *regs.gpr_mut(28) = percpu::percpu_area_base(cpu_id) as _;

        tcb.tcb_write_all_registers(false, &mut regs)?;

        tcb.tcb_set_affinity(cpu_id as _)?;

        let task = Self {
            entry,
            stack: sp,
            capset,
            untyped,
            ipc_buffer_addr: virt,
            tid,
            affinity: cpu_id,
            cnode_index,
            migrate: false,
        };

        Ok(task)
    }

    pub fn start(&mut self) -> sel4::Result<()> {
        if self.migrate {
            sel4::init_thread::slot::CNODE
                .cap()
                .absolute_cptr_from_bits_with_depth(
                    (self.cnode_index << 12) as u64 + DEFAULT_PARENT_EP.bits(),
                    64,
                )
                .delete()?;

            sel4::init_thread::slot::CNODE
                .cap()
                .absolute_cptr_from_bits_with_depth(
                    (self.cnode_index << 12) as u64 + DEFAULT_PARENT_EP.bits(),
                    64,
                )
                .mint(
                    &LeafSlot::from(DEFAULT_SERVE_EP).abs_cptr(),
                    CapRights::all(),
                    self.tid as _,
                )?;

            let tcb = LeafSlot::new((self.cnode_index << CNODE_RADIX_BITS) + 1).cap();
            let mut regs = tcb.tcb_read_all_registers(true).unwrap();

            *regs.gpr_mut(28) = percpu::percpu_area_base(self.affinity) as _;

            tcb.tcb_write_all_registers(false, &mut regs).unwrap();
            tcb.tcb_set_affinity(self.affinity as _).unwrap();

            self.migrate = false;
        }

        LeafSlot::new((self.cnode_index << CNODE_RADIX_BITS) + 1)
            .cap()
            .tcb_resume()
    }

    pub fn migrate(&mut self, target: usize) -> sel4::Result<()> {
        if self.affinity == target {
            return Ok(());
        }

        self.affinity = target;
        self.migrate = true;

        Ok(())
    }

    pub fn suspend(&self) -> sel4::Result<()> {
        LeafSlot::new((self.cnode_index << CNODE_RADIX_BITS) + 1)
            .cap()
            .tcb_suspend()
    }

    pub fn exit(&self) {
        self.capset.exit();

        dealloc_ipc_buffer(self.ipc_buffer_addr);

        for i in 0..axconfig::plat::CPU_NUM {
            let _ = LeafSlot::new(0x90 + i)
                .cap()
                .absolute_cptr_from_bits_with_depth(self.cnode_index as _, 52)
                .revoke();

            let _ = LeafSlot::new(0x90 + i)
                .cap()
                .absolute_cptr_from_bits_with_depth(self.cnode_index as _, 52)
                .delete();
        }

        TASK_CSPACE_ALLOCATOR.lock().recycle(self.cnode_index);
    }
}

pub(crate) struct InitTask {
    pub tcb: cap::Tcb,
}

impl InitTask {
    pub fn new(entry: usize, stack: usize, affinity: usize) -> sel4::Result<Self> {
        let untyped = alloc_untyped_raw(19);
        let obj_allocator = ObjectAllocator::empty();
        obj_allocator.init(untyped);
        let tid = 0xF000 + affinity;

        // create a 2-level cspace
        let cnode = obj_allocator.alloc_cnode(CNODE_RADIX_BITS);
        let inner_cnode = obj_allocator.alloc_cnode(CNODE_RADIX_BITS);
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

        // create a endpoint for task
        let ep = obj_allocator.alloc_endpoint();

        cnode.absolute_cptr(init_thread::slot::CNODE.cptr()).mint(
            &LeafSlot::from_cap(cnode).abs_cptr(),
            CapRights::all(),
            CNodeCapData::skip_high_bits(2 * CNODE_RADIX_BITS).into_word(),
        )?;

        cnode
            .absolute_cptr_from_bits_with_depth(0x90 as _, 64)
            .copy(
                &LeafSlot::from_cap(sel4::init_thread::slot::CNODE.cap()).abs_cptr(),
                CapRights::all(),
            )
            .unwrap();

        init_thread::slot::CNODE
            .cap()
            .absolute_cptr_from_bits_with_depth((0x90 + affinity) as _, 64)
            .copy(&LeafSlot::from_cap(cnode).abs_cptr(), CapRights::all())
            .unwrap();

        init_thread::slot::CNODE
            .cap()
            .absolute_cptr_from_bits_with_depth((0x80 + affinity) as _, 64)
            .copy(&LeafSlot::from_cap(tcb).abs_cptr(), CapRights::all())
            .unwrap();

        // copy tcb into thread cspace
        cnode
            .absolute_cptr(init_thread::slot::TCB.cptr())
            .copy(&LeafSlot::from_cap(tcb).abs_cptr(), CapRights::all())?;

        // copy parent endpoint to child
        cnode.absolute_cptr(DEFAULT_PARENT_EP.cptr()).mint(
            &LeafSlot::from(DEFAULT_PARENT_EP).abs_cptr(),
            CapRights::all(),
            tid as _,
        )?;

        // copy srv endpoint to cnode
        cnode
            .absolute_cptr(DEFAULT_SERVE_EP.cptr())
            .copy(&LeafSlot::from_cap(ep).abs_cptr(), CapRights::all())?;

        let (virt, ipc_cap) = alloc_ipc_buffer(&obj_allocator).unwrap();

        // copy untyped into cnode
        let untyped_raw = alloc_untyped_raw(22);

        cnode.absolute_cptr_from_bits_with_depth(23, 64).copy(
            &LeafSlot::from_cap(untyped_raw).abs_cptr(),
            CapRights::all(),
        )?;

        // copy vspace to thread
        cnode.absolute_cptr(init_thread::slot::VSPACE.cptr()).copy(
            &LeafSlot::from(init_thread::slot::VSPACE.cap()).abs_cptr(),
            CapRights::all(),
        )?;

        tcb.tcb_configure(
            DEFAULT_PARENT_EP.cptr(),
            cnode,
            CNodeCapData::skip_high_bits(2 * CNODE_RADIX_BITS),
            init_thread::slot::VSPACE.cap(),
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

        Ok(Self { tcb })
    }

    #[allow(unused)]
    pub fn start(&self) -> sel4::Result<()> {
        self.tcb.tcb_resume()
    }
}

pub fn create_sel4_task(
    tid: usize,
    entry: usize,
    stack: usize,
    tls: usize,
    cpu_id: usize,
) -> usize {
    let t = Arc::new(SpinNoIrq::new(
        NormalTask::new(tid, entry, stack, 100, tls, cpu_id).unwrap(),
    ));
    TASK_MAP.lock().insert(tid, t);
    tid as usize
}

pub fn exit_sel4_task(tid: usize) {
    if let Some(t) = TASK_MAP.lock().get(&tid) {
        t.lock().exit();
    }
}

pub fn migrate_sel4_task(tid: usize, target: usize) {
    if let Some(t) = TASK_MAP.lock().get(&tid) {
        t.lock().migrate(target).unwrap();
    }
}

pub fn start_sel4_task(tid: usize) {
    if let Some(t) = TASK_MAP.lock().get(&tid) {
        t.lock().start().unwrap();
    }
}

pub fn suspend_sel4_task(tid: usize) {
    if let Some(t) = TASK_MAP.lock().get(&tid) {
        t.lock().suspend().unwrap();
    }
}

pub fn switch_sel4_task(prev_tid: usize, next_tid: usize) {
    if let Some(t) = TASK_MAP.lock().get(&prev_tid) {
        t.lock().suspend().unwrap();
    }

    if let Some(t) = TASK_MAP.lock().get(&next_tid) {
        t.lock().start().unwrap();
    }
}

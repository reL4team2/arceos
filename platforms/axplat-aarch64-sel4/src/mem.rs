//! This module provides the implementation of the memory interface for the seL4 platform.
//! It initializes the memory space, manages memory regions, and provides methods to map and allocate memory.
use axplat::mem::{MemIf, PhysAddr, RawRange, VirtAddr};
use common::root::translate_addr;

use crate::config::devices::MMIO_RANGES;
use kit::mem::MemCap;
use kit::obj::CapSet;
use lazyinit::LazyInit;
use sel4::cap;
use sel4::init_thread;

const MEM_START_ADDR: usize = axconfig::plat::VIRT_MEMORY_BASE;
const MEM_SIZE: usize = axconfig::plat::VIRT_MEMORY_SIZE;

const VIRT_FRAME_ADDR: usize = axconfig::plat::VIRT_FRAME_BASE;
const VIRT_FRAME_SIZE: usize = axconfig::plat::VIRT_FRAME_SIZE;

/// Global memory space manager for the seL4 platform.
pub(crate) static MEM_SPACE: LazyInit<MemCap> = LazyInit::new();

/// Initializes the memory space and sets up the global memory allocator.
pub(crate) fn init() {
    // TODO: use config to get the memory size
    // pre allocator initialize
    // axalloc::global_init(
    //     axconfig::plat::INIT_HEAP_BASE,
    //     axconfig::plat::INIT_HEAP_SIZE,
    // );
    // MEM_SPACE.init_once(MemSpace::new());
    // MEM_SPACE.init();
    // MEM_SPACE.map_area(MEM_START_ADDR, MEM_SIZE);

    let vspace = init_thread::slot::VSPACE.cap();
    let untyped = sel4::Cap::from_bits(24);
    MEM_SPACE.init_once(MemCap::new(
        vspace,
        untyped,
        VIRT_FRAME_ADDR,
        VIRT_FRAME_SIZE,
    ));
    MEM_SPACE
        .large_page_map_alloc(MEM_START_ADDR.into(), MEM_SIZE)
        .unwrap();

    let paddr = translate_addr(axconfig::plat::INIT_HEAP_BASE);
    MEM_SPACE.add_region(
        axconfig::plat::INIT_HEAP_BASE,
        paddr,
        axconfig::plat::INIT_HEAP_SIZE,
    );
}

/// allocate a IPC buffer for new create seL4 thread
pub(crate) fn alloc_ipc_buffer() -> sel4::Result<(VirtAddr, cap::Granule)> {
    MEM_SPACE.alloc_ipc_buffer(None)
}

pub(crate) fn alloc_ipc_buffer_by_capset(
    capset: &mut CapSet,
) -> sel4::Result<(VirtAddr, cap::Granule)> {
    MEM_SPACE.alloc_ipc_buffer(Some(capset))
}

pub(crate) fn dealloc_ipc_buffer(virt: VirtAddr) {
    MEM_SPACE.dealloc_ipc_buffer(virt);
}

struct MemIfImpl;

#[impl_plat_interface]
impl MemIf for MemIfImpl {
    /// Returns all physical memory (RAM) ranges on the platform.
    ///
    /// All memory ranges except reserved ranges (including the kernel loaded
    /// range) are free for allocation.
    fn phys_ram_ranges() -> &'static [RawRange] {
        // TODO: actually need return physical address
        &[(MEM_START_ADDR, MEM_SIZE)]
    }

    /// Returns all reserved physical memory ranges on the platform.
    ///
    /// Reserved memory can be contained in [`phys_ram_ranges`], they are not
    /// allocatable but should be mapped to kernel's address space.
    ///
    /// Note that the ranges returned should not include the range where the
    /// kernel is loaded.
    fn reserved_phys_ram_ranges() -> &'static [RawRange] {
        &[]
    }

    /// Returns all device memory (MMIO) ranges on the platform.
    fn mmio_ranges() -> &'static [RawRange] {
        &MMIO_RANGES
    }

    /// Translates a physical address to a virtual address.
    ///
    /// It is just an easy way to access physical memory when virtual memory
    /// is enabled. The mapping may not be unique, there can be multiple `vaddr`s
    /// mapped to that `paddr`.
    fn phys_to_virt(paddr: PhysAddr) -> VirtAddr {
        MEM_SPACE
            .phys_to_virt(paddr)
            .unwrap_or(VirtAddr::from_usize(paddr.as_usize()))
    }

    /// Translates a virtual address to a physical address.
    ///
    /// It is a reverse operation of [`phys_to_virt`]. It requires that the
    /// `vaddr` must be available through the [`phys_to_virt`] translation.
    /// It **cannot** be used to translate arbitrary virtual addresses.
    fn virt_to_phys(vaddr: VirtAddr) -> PhysAddr {
        MEM_SPACE
            .virt_to_phys(vaddr)
            .unwrap_or(PhysAddr::from_usize(vaddr.as_usize()))
    }
}

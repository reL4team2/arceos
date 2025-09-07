//! This module provides the implementation of the memory interface for the seL4 platform.
//! It initializes the memory space, manages memory regions, and provides methods to map and allocate memory.
use axplat::mem::{MemIf, PhysAddr, RawRange, VirtAddr};
use common::ObjectAllocator;
use common::root::translate_addr;

use crate::config::devices::MMIO_RANGES;
use crate::utils::obj::alloc_pt;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use kspin::SpinNoIrq;
use lazyinit::LazyInit;
use sel4::cap;

const MEM_START_ADDR: usize = axconfig::plat::VIRT_MEMORY_BASE;
const MEM_SIZE: usize = axconfig::plat::VIRT_MEMORY_SIZE;

const VIRT_FRAME_ADDR: usize = axconfig::plat::VIRT_FRAME_BASE;
const VIRT_FRAME_SIZE: usize = axconfig::plat::VIRT_FRAME_SIZE;

const LARGE_PAGE_SIZE: usize = 0x200000; // 2MB
const PAGE_SIZE: usize = 0x1000; // 4KB

/// Global memory space manager for the seL4 platform.
pub(crate) static MEM_SPACE: LazyInit<MemSpace> = LazyInit::new();

/// Represents a memory space in the seL4 platform.
/// The mem_allocator only retype to Large Page Cap, make sure the memory is consequently allocated.
pub(crate) struct MemSpace {
    pub(crate) regions: SpinNoIrq<BTreeMap<usize, RawRange>>,
    pub(crate) vspace: cap::VSpace,
    pub(crate) mem_allocator: ObjectAllocator,
    pub(crate) vp_allocator: SpinNoIrq<VirtFrameAllocator>,
}

impl MemSpace {
    pub(crate) const fn new() -> Self {
        MemSpace {
            regions: SpinNoIrq::new(BTreeMap::new()),
            vspace: sel4::init_thread::slot::VSPACE.cap(),
            mem_allocator: ObjectAllocator::empty(),
            vp_allocator: SpinNoIrq::new(VirtFrameAllocator::new()),
        }
    }

    /// Receive the untyped cap from root_task, and used for allocating memory.
    pub(crate) fn init(&self) {
        self.mem_allocator.init(sel4::Cap::from_bits(24));
        // add pre allocator heap region
        let paddr = translate_addr(axconfig::plat::INIT_HEAP_BASE);
        self.regions.lock().insert(
            axconfig::plat::INIT_HEAP_BASE,
            (paddr, paddr + axconfig::plat::INIT_HEAP_SIZE),
        );
    }

    /// Adds a memory region to the memory space.
    pub(crate) fn add_region(&self, vaddr: usize, paddr: usize, size: usize) {
        self.regions.lock().insert(vaddr, (paddr, paddr + size));
    }

    /// Maps a memory area to the virtual address space.
    pub(crate) fn map_area(&self, vaddr: usize, size: usize) {
        // only support large page map
        assert_eq!(vaddr % LARGE_PAGE_SIZE, 0);
        assert!(size > 0);

        let caps = self.mem_allocator.alloc_large_pages(size / LARGE_PAGE_SIZE);
        let mut total_size: usize = 0;
        let paddr = caps[0]
            .frame_get_address()
            .expect("can't get address of the physical page");
        for (i, cap) in caps.iter().enumerate() {
            let vaddr_offset = vaddr + i * LARGE_PAGE_SIZE;
            self.map_large_page(vaddr_offset, cap);
            total_size += LARGE_PAGE_SIZE;
        }

        self.add_region(vaddr, paddr, total_size);
    }

    fn map_page(&self, vaddr: usize, page: &self::cap::Granule, allocator: &ObjectAllocator) {
        assert_eq!(vaddr % PAGE_SIZE, 0);
        for _ in 0..sel4::vspace_levels::NUM_LEVELS {
            let res = page.frame_map(
                self.vspace,
                vaddr as _,
                sel4::CapRights::all(),
                sel4::VmAttributes::DEFAULT,
            );
            match res {
                Ok(_) => {
                    return;
                }
                Err(sel4::Error::FailedLookup) => {
                    let pt_cap = allocator.alloc_pt();
                    pt_cap
                        .pt_map(self.vspace, vaddr as _, sel4::VmAttributes::DEFAULT)
                        .unwrap();
                }
                _ => res.unwrap(),
            }
        }
        unreachable!("Failed to map page at vaddr {:#x}", vaddr);
    }

    fn map_large_page(&self, vaddr: usize, page: &sel4::cap::LargePage) {
        assert_eq!(vaddr % LARGE_PAGE_SIZE, 0);
        for _ in 0..sel4::vspace_levels::NUM_LEVELS {
            let res = page.frame_map(
                self.vspace,
                vaddr as _,
                sel4::CapRights::all(),
                sel4::VmAttributes::DEFAULT,
            );
            match res {
                Ok(_) => {
                    return;
                }
                Err(sel4::Error::FailedLookup) => {
                    let pt_cap = alloc_pt();
                    pt_cap
                        .pt_map(self.vspace, vaddr as _, sel4::VmAttributes::DEFAULT)
                        .unwrap();
                }
                _ => res.unwrap(),
            }
        }
        unreachable!("Failed to map large page at vaddr {:#x}", vaddr);
    }

    fn virt_to_phys(&self, vaddr: usize) -> usize {
        let vstart = (vaddr / LARGE_PAGE_SIZE) * LARGE_PAGE_SIZE;
        if let Some(range) = self.regions.lock().get(&vstart) {
            let pstart = range.0;
            return pstart + (vaddr - vstart);
        }

        vaddr
    }

    fn phys_to_virt(&self, paddr: usize) -> usize {
        for (vstart, range) in self.regions.lock().iter() {
            if range.0 <= paddr && paddr < range.1 {
                return vstart + (paddr - range.0);
            }
        }

        paddr
    }

    fn alloc_ipc_buffer(
        &self,
        allocator: &ObjectAllocator,
    ) -> sel4::Result<(usize, sel4::cap::Granule)> {
        // Allocate an IPC buffer at a fixed address.
        let ipc_vpn = self
            .vp_allocator
            .lock()
            .alloc()
            .ok_or(sel4::Error::NotEnoughMemory)?;
        let ipc_cap = allocator.alloc_page();
        self.map_page(ipc_vpn * PAGE_SIZE, &ipc_cap, allocator);
        Ok((ipc_vpn * PAGE_SIZE, ipc_cap))
    }

    fn dealloc_ipc_buffer(&self, vpn: usize) {
        self.vp_allocator.lock().dealloc(vpn);
    }
}

pub(crate) struct VirtFrameAllocator {
    current: usize,
    end: usize,
    recycled: Vec<usize>,
}

impl VirtFrameAllocator {
    pub(crate) const fn new() -> Self {
        VirtFrameAllocator {
            current: VIRT_FRAME_ADDR / PAGE_SIZE,
            end: (VIRT_FRAME_ADDR + VIRT_FRAME_SIZE) / PAGE_SIZE,
            recycled: Vec::new(),
        }
    }

    pub(crate) fn alloc(&mut self) -> Option<usize> {
        if self.current == self.end {
            if let Some(vpn) = self.recycled.pop() {
                return Some(vpn);
            }
            return None;
        } else {
            let vpn = self.current;
            self.current += 1;
            return Some(vpn);
        }
    }

    #[allow(unused)]
    pub(crate) fn dealloc(&mut self, vpn: usize) {
        if vpn < self.current && !self.recycled.contains(&vpn) {
            self.recycled.push(vpn);
        }
    }
}

/// Initializes the memory space and sets up the global memory allocator.
pub(crate) fn init() {
    // TODO: use config to get the memory size
    // pre allocator initialize
    // axalloc::global_init(
    //     axconfig::plat::INIT_HEAP_BASE,
    //     axconfig::plat::INIT_HEAP_SIZE,
    // );
    MEM_SPACE.init_once(MemSpace::new());
    MEM_SPACE.init();
    MEM_SPACE.map_area(MEM_START_ADDR, MEM_SIZE);
}

/// allocate a IPC buffer for new create seL4 thread
pub(crate) fn alloc_ipc_buffer(
    allocator: &ObjectAllocator,
) -> sel4::Result<(usize, sel4::cap::Granule)> {
    MEM_SPACE.alloc_ipc_buffer(allocator)
}

pub(crate) fn dealloc_ipc_buffer(virt: usize) {
    MEM_SPACE.dealloc_ipc_buffer(virt / PAGE_SIZE);
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
        MEM_SPACE.phys_to_virt(paddr.into()).into()
    }

    /// Translates a virtual address to a physical address.
    ///
    /// It is a reverse operation of [`phys_to_virt`]. It requires that the
    /// `vaddr` must be available through the [`phys_to_virt`] translation.
    /// It **cannot** be used to translate arbitrary virtual addresses.
    fn virt_to_phys(vaddr: VirtAddr) -> PhysAddr {
        MEM_SPACE.virt_to_phys(vaddr.into()).into()
    }
}

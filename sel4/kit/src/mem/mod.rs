use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use kspin::SpinNoIrq;
use memory_addr::{AddrRange, MemoryAddr, PhysAddr, VirtAddr};
use sel4::CapTypeForFrameObject;
use sel4::cap::{Granule, Untyped, VSpace};
use sel4::{Cap, cap_type};

use crate::config::*;
use crate::obj::allocator::VirtFrameAllocator;
use crate::obj::{CapSet, MemCapAllocator};
use common::ObjectAllocator;

pub struct MemCapGlobalAllocator {
    obj_allocator: ObjectAllocator,
}

impl MemCapGlobalAllocator {
    fn new(untyped: Untyped) -> Self {
        let obj_allocator = ObjectAllocator::empty();
        obj_allocator.init(untyped);
        MemCapGlobalAllocator { obj_allocator }
    }
}

impl MemCapAllocator for MemCapGlobalAllocator {
    fn alloc_pt(&self) -> sel4::Result<Cap<cap_type::PT>> {
        Ok(self.obj_allocator.alloc_pt())
    }

    fn alloc_page(&self) -> sel4::Result<Cap<cap_type::Granule>> {
        Ok(self.obj_allocator.alloc_page())
    }

    fn alloc_pages(&self, count: usize) -> sel4::Result<Vec<Cap<cap_type::Granule>>> {
        Ok(self.obj_allocator.alloc_pages(count))
    }

    fn alloc_large_page(&self) -> sel4::Result<Cap<cap_type::LargePage>> {
        Ok(self.obj_allocator.alloc_large_page())
    }

    fn alloc_large_pages(&self, count: usize) -> sel4::Result<Vec<Cap<cap_type::LargePage>>> {
        Ok(self.obj_allocator.alloc_large_pages(count))
    }
}

pub struct MemCap {
    vspace: VSpace,
    obj_allocator: MemCapGlobalAllocator,

    v2p_map: SpinNoIrq<BTreeMap<usize, AddrRange<usize>>>,
    p2v_map: SpinNoIrq<BTreeMap<usize, AddrRange<usize>>>,

    frame_allocator: SpinNoIrq<VirtFrameAllocator>,
}

impl MemCap {
    pub fn new(vspace: VSpace, untyped: Untyped, frame_start: usize, frame_size: usize) -> Self {
        let obj_allocator = MemCapGlobalAllocator::new(untyped);
        let v2p_map = SpinNoIrq::new(BTreeMap::new());
        let p2v_map = SpinNoIrq::new(BTreeMap::new());
        let frame_allocator = SpinNoIrq::new(VirtFrameAllocator::new(frame_start, frame_size));

        MemCap {
            vspace,
            obj_allocator,
            v2p_map,
            p2v_map,
            frame_allocator,
        }
    }

    pub fn large_page_map_alloc(&self, start: VirtAddr, size: usize) -> sel4::Result<PhysAddr> {
        if !start.is_aligned(LARGE_PAGE_SIZE) {
            return Err(sel4::Error::AlignmentError);
        }

        if size == 0 || (size % LARGE_PAGE_SIZE != 0) {
            return Err(sel4::Error::AlignmentError);
        }

        let caps = self
            .obj_allocator
            .alloc_large_pages(size / LARGE_PAGE_SIZE)?;
        let paddr = caps[0].frame_get_address()?;

        for (i, cap) in caps.iter().enumerate() {
            let vaddr_offset = start.add(i * LARGE_PAGE_SIZE);
            self.map_page::<cap_type::LargePage, MemCapGlobalAllocator>(
                vaddr_offset,
                cap,
                &self.obj_allocator,
            )?;
        }

        self.add_region(start.as_usize(), paddr, size);

        Ok(PhysAddr::from_usize(paddr))
    }

    pub fn map_alloc(&self, start: VirtAddr, size: usize) -> sel4::Result<PhysAddr> {
        if !start.is_aligned(PAGE_SIZE) {
            return Err(sel4::Error::AlignmentError);
        }

        if (size % PAGE_SIZE != 0) || (size == 0) {
            return Err(sel4::Error::AlignmentError);
        }

        let caps = self.obj_allocator.alloc_pages(size / PAGE_SIZE)?;
        let paddr = caps[0].frame_get_address()?;

        for (i, cap) in caps.iter().enumerate() {
            let vaddr_offset = start.add(i * PAGE_SIZE);
            self.map_page::<cap_type::Granule, MemCapGlobalAllocator>(
                vaddr_offset,
                cap,
                &self.obj_allocator,
            )?;
        }

        self.add_region(start.as_usize(), paddr, size);

        Ok(PhysAddr::from_usize(paddr))
    }

    pub fn virt_to_phys(&self, vaddr: VirtAddr) -> sel4::Result<PhysAddr> {
        let lp_vstart = (vaddr.as_usize() / LARGE_PAGE_SIZE) * LARGE_PAGE_SIZE;
        let vstart = (vaddr.as_usize() / PAGE_SIZE) * PAGE_SIZE;

        if let Some(range) = self.v2p_map.lock().get(&lp_vstart) {
            let paddr = range.start + (vaddr.as_usize() - lp_vstart);
            if paddr < range.end {
                return Ok(PhysAddr::from_usize(paddr));
            }
        } else if let Some(range) = self.v2p_map.lock().get(&vstart) {
            let paddr = range.start + (vaddr.as_usize() - lp_vstart);
            if paddr < range.end {
                return Ok(PhysAddr::from_usize(paddr));
            }
        }

        return Err(sel4::Error::FailedLookup);
    }

    pub fn phys_to_virt(&self, paddr: PhysAddr) -> sel4::Result<VirtAddr> {
        let lp_pstart = (paddr.as_usize() / LARGE_PAGE_SIZE) * LARGE_PAGE_SIZE;
        let pstart = (paddr.as_usize() / PAGE_SIZE) * PAGE_SIZE;

        if let Some(range) = self.p2v_map.lock().get(&lp_pstart) {
            let vaddr = range.start + (paddr.as_usize() - lp_pstart);
            if vaddr < range.end {
                return Ok(VirtAddr::from_usize(vaddr));
            }
        } else if let Some(range) = self.p2v_map.lock().get(&pstart) {
            let vaddr = range.start + (paddr.as_usize() - pstart);
            if vaddr < range.end {
                return Ok(VirtAddr::from_usize(vaddr));
            }
        }

        return Err(sel4::Error::FailedLookup);
    }

    pub fn alloc_ipc_buffer(
        &self,
        allocator: Option<&mut CapSet>,
    ) -> sel4::Result<(VirtAddr, Granule)> {
        let ipc_vpn = self
            .frame_allocator
            .lock()
            .alloc()
            .ok_or(sel4::Error::NotEnoughMemory)?;

        let page_cap: Granule = match allocator {
            Some(alloc) => {
                let ipc_cap = alloc.alloc_page()?;
                self.map_page::<cap_type::Granule, CapSet>(
                    VirtAddr::from_usize(ipc_vpn * PAGE_SIZE),
                    &ipc_cap,
                    alloc,
                )?;
                ipc_cap
            }
            None => {
                let ipc_cap = self.obj_allocator.alloc_page()?;
                self.map_page::<cap_type::Granule, MemCapGlobalAllocator>(
                    VirtAddr::from_usize(ipc_vpn * PAGE_SIZE),
                    &ipc_cap,
                    &self.obj_allocator,
                )?;
                ipc_cap
            }
        };

        Ok((VirtAddr::from_usize(ipc_vpn * PAGE_SIZE), page_cap))
    }

    pub fn dealloc_ipc_buffer(&self, vaddr: VirtAddr) {
        let vpn = vaddr.as_usize() / PAGE_SIZE;
        self.frame_allocator.lock().dealloc(vpn);
    }

    pub fn add_region(&self, vaddr: usize, paddr: usize, size: usize) {
        self.v2p_map
            .lock()
            .insert(vaddr, AddrRange::new(paddr, paddr + size));
        self.p2v_map
            .lock()
            .insert(paddr, AddrRange::new(vaddr, vaddr + size));
    }

    fn map_page<T: CapTypeForFrameObject, C: MemCapAllocator>(
        &self,
        vaddr: VirtAddr,
        page: &Cap<T>,
        caps_alloc: &C,
    ) -> sel4::Result<()> {
        for _ in 0..sel4::vspace_levels::NUM_LEVELS {
            let res = page.frame_map(
                self.vspace,
                vaddr.as_usize() as _,
                sel4::CapRights::all(),
                sel4::VmAttributes::DEFAULT,
            );
            match res {
                Ok(_) => {
                    return Ok(());
                }
                Err(sel4::Error::FailedLookup) => {
                    let pt_cap = caps_alloc.alloc_pt()?;
                    pt_cap.pt_map(
                        self.vspace,
                        vaddr.as_usize() as _,
                        sel4::VmAttributes::DEFAULT,
                    )?;
                }
                _ => {
                    return res;
                }
            }
        }
        unreachable!("Failed to map large page at vaddr {:#x}", vaddr.as_usize());
    }
}

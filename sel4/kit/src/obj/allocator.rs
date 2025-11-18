use crate::config::*;
use alloc::vec::Vec;

pub struct IndexAllocator {
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
        if self.next < self.max {
            let index = self.next;
            self.next += 1;
            Some(index)
        } else if let Some(index) = self.recycled.pop() {
            Some(index as usize)
        } else {
            None
        }
    }

    pub fn recycle(&mut self, index: usize) {
        self.recycled.push(index as u64);
    }
}

pub(crate) struct VirtFrameAllocator {
    current: usize,
    end: usize,
    recycled: Vec<usize>,
}

impl VirtFrameAllocator {
    pub(crate) const fn new(vstart: usize, size: usize) -> Self {
        VirtFrameAllocator {
            current: vstart / PAGE_SIZE,
            end: (vstart + size) / PAGE_SIZE,
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

use alloc::vec;
use common::ObjectAllocator;

pub struct UntypedAllocator<'a> {
    recycled: Vec<Vec<sel4::cap::Untyped>>,
    obj_allocator: &'a ObjectAllocator,
    untyped_size: usize,
}

impl<'a> UntypedAllocator<'a> {
    pub fn new(obj_allocator: &'a ObjectAllocator, untyped_size: usize, cpu_num: usize) -> Self {
        UntypedAllocator {
            recycled: vec![Vec::new(); cpu_num],
            obj_allocator,
            untyped_size,
        }
    }

    pub fn alloc(&mut self, cpu_id: usize) -> sel4::cap::Untyped {
        if let Some(cap) = self.recycled[cpu_id].pop() {
            cap
        } else {
            self.obj_allocator.alloc_untyped(self.untyped_size)
        }
    }

    pub fn dealloc(&mut self, cap: sel4::cap::Untyped, cpu_id: usize) {
        self.recycled[cpu_id].push(cap);
    }
}

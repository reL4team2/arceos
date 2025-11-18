use sel4::cap::Endpoint;

pub(crate) const CSPACE_DEPTH: usize = 64;
pub(crate) const DEFAULT_PARENT_EP: Endpoint = Endpoint::from_bits(18);
pub(crate) const LARGE_PAGE_SIZE: usize = 0x200000; // 2MB
pub(crate) const PAGE_SIZE: usize = 0x1000; // 4KB
#[cfg(feature = "irq")]
pub(crate) const PPI_NUM: usize = 32;

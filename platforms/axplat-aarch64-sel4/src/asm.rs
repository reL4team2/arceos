use memory_addr::PhysAddr;

pub unsafe fn write_kernel_page_table(_root_paddr: PhysAddr) {}

#[cfg(feature = "irq")]
pub use crate::irq::{disable_irqs, enable_irqs, irqs_enabled};

#[cfg(feature = "irq")]
pub fn wait_for_irqs() {
    sel4::r#yield();
}

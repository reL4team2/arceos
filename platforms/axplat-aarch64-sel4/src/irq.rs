//! This module provides the implementation of the IRQ interface for the seL4 platform.
//! It initializes the IRQ handler, registers IRQs, and provides methods to enable/disable
use axplat::irq::{HandlerTable, IpiTarget, IrqHandler, IrqIf};
use lazyinit::LazyInit;

use sel4::cap::Notification;
use sel4_oskit::irq::IrqManager;

const MAX_IRQ_COUNT: usize = 1024;

#[percpu::def_percpu]
static IRQ_HANDLER_TABLE: HandlerTable<MAX_IRQ_COUNT> = HandlerTable::new();

#[percpu::def_percpu]
static IRQ_CAPS: LazyInit<IrqManager<MAX_IRQ_COUNT>> = LazyInit::new();

#[percpu::def_percpu]
static IRQ_ENABLED: bool = false;

#[allow(unused_macros)]
macro_rules! handle_trap {
    ($trap:ident, $($args:tt)*) => {{
        let mut iter = axcpu::trap::$trap.iter();
        if let Some(func) = iter.next() {
            if iter.next().is_some() {
                log::warn!("Multiple handlers for trap {} are not currently supported", stringify!($trap));
            }
            func($($args)*)
        } else {
            log::warn!("No registered handler for trap {}", stringify!($trap));
            false
        }
    }}
}

pub(crate) fn init_early(cpu_id: usize) {
    IRQ_CAPS.with_current(|irq_cap| {
        irq_cap.init_once(IrqManager::new(cpu_id, unsafe {
            crate::obj::OBJ_ALLOCATOR.current_ref_raw()
        }));
    });
}

pub(crate) fn init_later(cpu: usize) {
    IRQ_CAPS.with_current(|irq_cap| {
        irq_cap.init(cpu).unwrap();
    });
}

pub fn handle_irq(badge: usize) {
    if irqs_enabled() {
        handle_trap!(IRQ, badge as _);
    }

    IRQ_CAPS.with_current(|irq_cap| {
        irq_cap.ack_irq(badge as _);
    });
}

#[inline(always)]
pub fn irqs_enabled() -> bool {
    IRQ_ENABLED.read_current()
}

#[inline(always)]
pub fn enable_irqs() {
    unsafe {
        IRQ_ENABLED.write_current_raw(true);
    }
}

#[inline(always)]
pub fn disable_irqs() {
    unsafe {
        IRQ_ENABLED.write_current_raw(false);
    }
}

struct IrqIfImpl;

/// Implementation of the Arceos IRQ interface for the seL4 platform.
/// Arceos system can use these interface without change.
#[impl_plat_interface]
impl IrqIf for IrqIfImpl {
    /// Enables or disables the given IRQ.
    fn set_enable(irq: usize, enabled: bool) {
        if enabled {
            IRQ_CAPS.with_current(|irq_cap| {
                irq_cap.register_irq(irq).unwrap();
            });
        } else {
            log::warn!(
                "Disabling IRQ on seL4 platform {} is not supported now!",
                irq
            );
        }
    }

    /// Registers an IRQ handler for the given IRQ.
    ///
    /// It also enables the IRQ if the registration succeeds. It returns `false`
    /// if the registration failed.
    fn register(irq: usize, handler: IrqHandler) -> bool {
        if unsafe { IRQ_HANDLER_TABLE.current_ref_mut_raw() }.register_handler(irq as _, handler) {
            IRQ_CAPS.with_current(|irq_cap| {
                irq_cap.register_irq(irq).unwrap();
            });
            return true;
        }

        false
    }

    /// Unregisters the IRQ handler for the given IRQ.
    ///
    /// It also disables the IRQ if the unregistration succeeds. It returns the
    /// existing handler if it is registered, `None` otherwise.
    fn unregister(irq: usize) -> Option<IrqHandler> {
        IRQ_CAPS.with_current(|irq_cap| {
            irq_cap
                .unregister_irq(irq, Notification::from_bits(0))
                .unwrap();
        });
        unsafe { IRQ_HANDLER_TABLE.current_ref_mut_raw() }.unregister_handler(irq as _)
    }

    /// Handles the IRQ.
    ///
    /// It is called by the common interrupt handler. It should look up in the
    /// IRQ handler table and calls the corresponding handler. If necessary, it
    /// also acknowledges the interrupt controller after handling.
    fn handle(irq: usize) {
        if !unsafe { IRQ_HANDLER_TABLE.current_ref_mut_raw() }.handle(irq as _) {
            log::warn!("Unhandled IRQ {}", irq);
        }
    }

    /// Sends an inter-processor interrupt (IPI) to the specified target CPU or all CPUs.
    fn send_ipi(_irq_num: usize, _target: IpiTarget) {}
}

use sel4_if::Sel4IrqIf;

#[impl_plat_interface]
impl Sel4IrqIf for IrqIfImpl {
    fn disable_irqs() {
        disable_irqs();
    }

    fn enable_irqs() {
        enable_irqs();
    }

    fn irqs_enabled() -> bool {
        irqs_enabled()
    }
}

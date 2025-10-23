//! This module provides the implementation of the IRQ interface for the seL4 platform.
//! It initializes the IRQ handler, registers IRQs, and provides methods to enable/disable
use axplat::irq::{HandlerTable, IpiTarget, IrqHandler, IrqIf};
use kspin::SpinNoIrq;
use lazyinit::LazyInit;

// sel4 crates
use alloc::collections::BTreeMap;

use common::{root::register_irq, slot::alloc_slot};
use sel4::cap::{IrqHandler as Sel4IrqHandler, Notification};
use sel4_kit::slot_manager::LeafSlot;

use crate::utils::obj::alloc_notification;

const MAX_IRQ_COUNT: usize = 1024;

#[percpu::def_percpu]
static IRQ_HANDLER_TABLE: HandlerTable<MAX_IRQ_COUNT> = HandlerTable::new();

#[percpu::def_percpu]
static IRQ_CAPS: LazyInit<SpinNoIrq<IrqCap>> = LazyInit::new();

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
        irq_cap.init_once(SpinNoIrq::new(IrqCap::new(cpu_id)));
    });
}

pub(crate) fn init_later(cpu: usize) {
    IRQ_CAPS.with_current(|irq_cap| {
        irq_cap.lock().init(cpu).unwrap();
    });
}

pub fn handle_irq(badge: usize) {
    handle_trap!(IRQ, badge as _);

    IRQ_CAPS.with_current(|irq_cap| {
        irq_cap.lock().ack_irq(badge as _);
    });
}

#[inline(always)]
pub fn irqs_enabled() -> bool {
    unsafe { IRQ_CAPS.current_ref_mut_raw() }
        .lock()
        .irqs_enabled()
}

#[inline(always)]
pub fn enable_irqs() {
    IRQ_CAPS.with_current(|irq_cap| {
        irq_cap.lock().enable_irqs();
    });
}

#[inline(always)]
pub fn disable_irqs() {
    IRQ_CAPS.with_current(|irq_cap| {
        irq_cap.lock().disable_irqs();
    });
}

/// Represents the IRQ capabilities and handlers for the seL4 platform.
/// It manages the global notification for IRQs, the IRQ handlers, and the task that handles IRQs
struct IrqCap {
    enable: bool,
    global_notify: Notification,
    irq_handlers: BTreeMap<usize, Sel4IrqHandler>,
    cpu_id: usize,
}

impl IrqCap {
    /// Create a new instance of `IrqCap`.
    pub(crate) fn new(cpu_id: usize) -> Self {
        let global_notify = Notification::from_bits(0);
        let irq_handlers = BTreeMap::new();
        Self {
            enable: false,
            global_notify,
            irq_handlers,
            cpu_id,
        }
    }

    /// Initializes the IRQ capabilities and task.
    pub(crate) fn init(&mut self, cpu: usize) -> sel4::Result<()> {
        // create a global notification for IRQs
        self.global_notify = alloc_notification();
        self.enable = true;

        sel4::init_thread::slot::TCB
            .cap()
            .tcb_bind_notification(self.global_notify)?;
        log::info!("init irq notification on cpu {}", cpu);
        Ok(())
    }

    pub(crate) fn enable_irqs(&mut self) {
        self.enable = true;
    }

    pub(crate) fn disable_irqs(&mut self) {
        self.enable = false;
    }

    pub(crate) fn irqs_enabled(&self) -> bool {
        self.enable
    }

    /// Registers a seL4 IRQ and sets up the necessary capabilities and notifications.
    pub fn register_sel4_irq(&mut self, irq: usize) -> sel4::Result<()> {
        // create a notification for the IRQ
        // TODO: now only support PPI IRQ, consider SPI IRQ
        let idx = self.cpu_id * 32 + irq;
        let slot = alloc_slot();
        LeafSlot::from_cap(self.global_notify).mint_to(slot, sel4::CapRights::all(), irq as _)?;
        let notify = slot.cap();

        // create an IRQ handler
        let irq_handler = alloc_slot().cap();
        register_irq(idx as _, irq_handler.into());

        // set up the IRQ handler
        irq_handler.irq_handler_set_notification(notify)?;
        irq_handler.irq_handler_ack()?;
        self.irq_handlers.insert(irq, irq_handler);

        Ok(())
    }

    pub fn remove_sel4_irq(&mut self, idx: usize) {
        self.irq_handlers.remove(&idx);
    }

    pub fn ack_irq(&self, idx: usize) {
        self.irq_handlers
            .get(&idx)
            .map(|handler| handler.irq_handler_ack().unwrap());
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
                irq_cap.lock().register_sel4_irq(irq).unwrap();
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
                irq_cap.lock().register_sel4_irq(irq).unwrap();
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
            irq_cap.lock().remove_sel4_irq(irq);
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

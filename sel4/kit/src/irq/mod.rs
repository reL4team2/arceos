use alloc::collections::BTreeMap;
use sel4::cap::Notification;

use common::ObjectAllocator;
use common::{
    root::register_irq,
    slot::{alloc_slot, recycle_slot},
};
use sel4_kit::slot_manager::LeafSlot;

use crate::config::*;

#[linkme::distributed_slice]
pub static SEL4_IRQ: [fn(usize) -> bool];

pub struct IrqCap<'a> {
    enable: bool,
    global_notify: Notification,
    irq_handlers: BTreeMap<usize, (LeafSlot, LeafSlot)>,
    cpu_id: usize,
    obj_allocator: &'a ObjectAllocator,
}

impl<'a> IrqCap<'a> {
    pub fn new(cpu_id: usize, obj_allocator: &'a ObjectAllocator) -> Self {
        IrqCap {
            enable: false,
            global_notify: Notification::from_bits(0),
            irq_handlers: BTreeMap::new(),
            cpu_id,
            obj_allocator,
        }
    }

    pub fn init(&mut self, _cpu: usize) -> sel4::Result<()> {
        // create a global notification for IRQs
        // TODO: ask task have a default global notification
        self.global_notify = self.obj_allocator.alloc_notification();
        self.enable = true;

        sel4::init_thread::slot::TCB
            .cap()
            .tcb_bind_notification(self.global_notify)?;
        Ok(())
    }

    pub fn enable_irqs(&mut self) {
        self.enable = true;
    }

    pub fn disable_irqs(&mut self) {
        self.enable = false;
    }

    pub fn irqs_enabled(&self) -> bool {
        self.enable
    }

    pub fn register(&mut self, irq: usize) -> sel4::Result<()> {
        let idx = self.cpu_id * PPI_NUM + irq;
        let notify_slot = alloc_slot();
        LeafSlot::from_cap(self.global_notify).mint_to(
            notify_slot,
            sel4::CapRights::all(),
            irq as _,
        )?;

        let irq_slot = alloc_slot();
        register_irq(idx as _, irq_slot);

        irq_slot
            .cap()
            .irq_handler_set_notification(notify_slot.cap())?;
        irq_slot.cap().irq_handler_ack()?;
        self.irq_handlers.insert(irq, (irq_slot, notify_slot));

        Ok(())
    }

    pub fn unregister(&mut self, irq: usize) -> sel4::Result<()> {
        if let Some((irq_slot, notify_slot)) = self.irq_handlers.remove(&irq) {
            irq_slot.delete()?;
            notify_slot.delete()?;
            recycle_slot(irq_slot);
            recycle_slot(notify_slot);
        }
        Ok(())
    }

    pub fn ack_irq(&self, idx: usize) {
        self.irq_handlers
            .get(&idx)
            .map(|handler| handler.0.cap().irq_handler_ack().unwrap());
    }
}

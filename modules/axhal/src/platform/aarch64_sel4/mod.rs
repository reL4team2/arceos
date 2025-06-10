#![allow(unused_variables)]
#![allow(dead_code)]

pub mod boot;
pub mod pl011;

pub mod console;

pub mod misc {
    /// Shutdown the whole system, including all CPUs.
    pub fn terminate() -> ! {
        common::root::shutdown()
    }
}

#[cfg(feature = "smp")]
pub mod mp {
    /// Starts the given secondary CPU with its boot stack.
    pub fn start_secondary_cpu(cpu_id: usize, stack_top: crate::mem::PhysAddr) {}
}

pub mod mem {
    /// Returns platform-specific memory regions.
    pub(crate) fn platform_regions() -> impl Iterator<Item = crate::mem::MemRegion> {
        use crate::mem::MemRegionFlags;
        core::iter::once(crate::mem::MemRegion {
            paddr: pa!(0x1_0000_3000),
            size: 0x200000,
            flags: MemRegionFlags::FREE | MemRegionFlags::READ | MemRegionFlags::WRITE,
            name: "free memory",
        })
        .chain(crate::mem::default_mmio_regions())
        // core::iter::empty()
    }
}

pub mod time {
    /// Returns the current clock time in hardware ticks.
    pub fn current_ticks() -> u64 {
        0
    }

    /// Converts hardware ticks to nanoseconds.
    pub fn ticks_to_nanos(ticks: u64) -> u64 {
        ticks
    }

    /// Converts nanoseconds to hardware ticks.
    pub fn nanos_to_ticks(nanos: u64) -> u64 {
        nanos
    }

    /// Set a one-shot timer.
    ///
    /// A timer interrupt will be triggered at the specified monotonic time deadline (in nanoseconds).
    pub fn set_oneshot_timer(deadline_ns: u64) {}

    /// Return epoch offset in nanoseconds (wall time offset to monotonic clock start).
    pub fn epochoffset_nanos() -> u64 {
        0
    }
}

#[cfg(feature = "irq")]
pub mod irq {
    /// The maximum number of IRQs.
    pub const MAX_IRQ_COUNT: usize = 256;

    /// The timer IRQ number.
    pub const TIMER_IRQ_NUM: usize = 0;

    /// Enables or disables the given IRQ.
    pub fn set_enable(irq_num: usize, enabled: bool) {}

    /// Registers an IRQ handler for the given IRQ.
    pub fn register_handler(irq_num: usize, handler: crate::irq::IrqHandler) -> bool {
        false
    }

    /// Dispatches the IRQ.
    ///
    /// This function is called by the common interrupt handler. It looks
    /// up in the IRQ handler table and calls the corresponding handler. If
    /// necessary, it also acknowledges the interrupt controller after handling.
    pub fn dispatch_irq(irq_num: usize) {}
}

/// Initializes the platform devices for the primary CPU.
pub fn platform_init() {}

/// Initializes the platform devices for secondary CPUs.
#[cfg(feature = "smp")]
pub fn platform_init_secondary() {}

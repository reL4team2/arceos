use axplat::time::TimeIf;
use aarch64_cpu::registers::{CNTPCT_EL0, CNTFRQ_EL0};
use aarch64_cpu::registers::Readable;
use int_ratio::Ratio;

static mut CNTPCT_TO_NANOS_RATIO: Ratio = Ratio::zero();
static mut NANOS_TO_CNTPCT_RATIO: Ratio = Ratio::zero();

/// Converts hardware ticks to nanoseconds.
#[inline]
pub fn ticks_to_nanos(ticks: u64) -> u64 {
    unsafe { CNTPCT_TO_NANOS_RATIO.mul_trunc(ticks) }
}

/// Converts nanoseconds to hardware ticks.
#[inline]
pub fn nanos_to_ticks(nanos: u64) -> u64 {
    unsafe { NANOS_TO_CNTPCT_RATIO.mul_trunc(nanos) }
}

/// Early stage initialization: stores the timer frequency.
pub fn init_early() {
    let freq = CNTFRQ_EL0.get();
    unsafe {
        CNTPCT_TO_NANOS_RATIO = Ratio::new(axplat::time::NANOS_PER_SEC as u32, freq as u32);
        NANOS_TO_CNTPCT_RATIO = CNTPCT_TO_NANOS_RATIO.inverse();
    }
}

struct TimeIfImpl;

#[impl_plat_interface]
impl TimeIf for TimeIfImpl {
    /// Returns the current clock time in hardware ticks.
    fn current_ticks() -> u64 {
        CNTPCT_EL0.get()
    }

    /// Converts hardware ticks to nanoseconds.
    fn ticks_to_nanos(ticks: u64) -> u64 {
        ticks_to_nanos(ticks)
    }

    /// Converts nanoseconds to hardware ticks.
    fn nanos_to_ticks(nanos: u64) -> u64 {
        nanos_to_ticks(nanos)
    }

    /// Return epoch offset in nanoseconds (wall time offset to monotonic
    /// clock start).
    fn epochoffset_nanos() -> u64 {
        0
    }

    /// Set a one-shot timer.
    ///
    /// A timer interrupt will be triggered at the specified monotonic time
    /// deadline (in nanoseconds).
    #[cfg(feature = "irq")]
    fn set_oneshot_timer(deadline_ns: u64) {
        sel4_kit::arch::set_timer(core::time::Duration::from_nanos(deadline_ns));
    }
}

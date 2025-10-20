use crate::task::InitTask;
use axplat::power::PowerIf;
use common_macros::sel4_thread_entry;

use axconfig::{TASK_STACK_SIZE, plat::CPU_NUM};
use sel4_kit::slot_manager::LeafSlot;

struct PowerImpl;

#[unsafe(link_section = ".bss.stack")]
static mut SECONDARY_BOOT_STACK: [[u8; TASK_STACK_SIZE]; CPU_NUM - 1] =
    [[0; TASK_STACK_SIZE]; CPU_NUM - 1];

#[cfg(feature = "smp")]
pub(crate) fn init_secondary_task() {
    for i in 1..CPU_NUM {
        let stack_top = unsafe { SECONDARY_BOOT_STACK[i - 1].as_ptr_range().end as usize };

        let entry = _start_secondary as usize;
        let _ = InitTask::new(entry, stack_top, i).unwrap();
    }
}

#[cfg(feature = "smp")]
#[sel4_thread_entry]
extern "C" fn _start_secondary(cpu_id: usize) -> ! {
    axplat::call_secondary_main(cpu_id)
}

#[impl_plat_interface]
impl PowerIf for PowerImpl {
    /// Bootstraps the given CPU core with the given initial stack (in physical
    /// address).
    ///
    /// Where `cpu_id` is the logical CPU ID (0, 1, ..., N-1, N is the number of
    /// CPU cores on the platform).
    #[cfg(feature = "smp")]
    fn cpu_boot(cpu_id: usize, _stack: usize) {
        // create a sel4 task and set affinity
        LeafSlot::new(0x80 + cpu_id).cap().tcb_resume().unwrap();
    }

    /// Shutdown the whole system.
    fn system_off() -> ! {
        common::root::shutdown()
    }
}

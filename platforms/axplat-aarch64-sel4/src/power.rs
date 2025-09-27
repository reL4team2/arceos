use crate::task::Sel4Task;
use axplat::power::PowerIf;
use common_macros::sel4_thread_entry;

struct PowerImpl;

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
    fn cpu_boot(cpu_id: usize, stack: usize) {
        // create a sel4 task and set affinity
        let entry = _start_secondary as usize;
        let task = Sel4Task::new_init_task(entry, stack, cpu_id).unwrap();
        task.start().unwrap();
    }

    /// Shutdown the whole system.
    fn system_off() -> ! {
        common::root::shutdown()
    }
}

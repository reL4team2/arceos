use core::sync::atomic::{AtomicUsize, Ordering};

use axconfig::{TASK_STACK_SIZE, plat::CPU_NUM};

#[cfg(not(feature = "onsel4"))]
use axhal::mem::{VirtAddr, virt_to_phys};

#[cfg(not(feature = "onsel4"))]
#[unsafe(link_section = ".bss.stack")]
static mut SECONDARY_BOOT_STACK: [[u8; TASK_STACK_SIZE]; CPU_NUM - 1] =
    [[0; TASK_STACK_SIZE]; CPU_NUM - 1];

static ENTERED_CPUS: AtomicUsize = AtomicUsize::new(1);

#[allow(clippy::absurd_extreme_comparisons)]
pub fn start_secondary_cpus(primary_cpu_id: usize) {
    let mut logic_cpu_id = 0;
    for i in 0..CPU_NUM {
        if i != primary_cpu_id && logic_cpu_id < CPU_NUM - 1 {
            #[cfg(not(feature = "onsel4"))]
            let stack_top = virt_to_phys(VirtAddr::from(unsafe {
                SECONDARY_BOOT_STACK[logic_cpu_id].as_ptr_range().end as usize
            }));

            debug!("starting CPU {}...", i);
            #[cfg(not(feature = "onsel4"))]
            axhal::power::cpu_boot(i, stack_top.as_usize());
            #[cfg(feature = "onsel4")]
            axhal::power::cpu_boot(i, 0);

            logic_cpu_id += 1;

            while ENTERED_CPUS.load(Ordering::Acquire) <= logic_cpu_id {
                core::hint::spin_loop();
            }
        }
    }
}

/// The main entry point of the ArceOS runtime for secondary cores.
///
/// It is called from the bootstrapping code in the specific platform crate.
#[axplat::secondary_main]
pub fn rust_main_secondary(cpu_id: usize) -> ! {
    axhal::init_percpu_secondary(cpu_id);
    axhal::init_early_secondary(cpu_id);

    ENTERED_CPUS.fetch_add(1, Ordering::Release);
    info!("Secondary CPU {} started.", cpu_id);

    #[cfg(all(feature = "paging", not(feature = "onsel4")))]
    axmm::init_memory_management_secondary();

    axhal::init_later_secondary(cpu_id);

    #[cfg(feature = "multitask")]
    axtask::init_scheduler_secondary();

    #[cfg(feature = "ipi")]
    axipi::init();

    info!("Secondary CPU {:x} init OK.", cpu_id);
    super::INITED_CPUS.fetch_add(1, Ordering::Release);

    while !super::is_init_ok() {
        core::hint::spin_loop();
    }

    #[cfg(all(feature = "irq", not(feature = "onsel4")))]
    axhal::asm::enable_irqs();
    #[cfg(all(feature = "irq", feature = "onsel4"))]
    super::init_interrupt();

    #[cfg(all(feature = "tls", not(feature = "multitask")))]
    super::init_tls();

    #[cfg(not(feature = "onsel4"))]
    {
        #[cfg(feature = "multitask")]
        axtask::run_idle();
        #[cfg(not(feature = "multitask"))]
        loop {
            axhal::asm::wait_for_irqs();
        }
    }

    #[cfg(feature = "onsel4")]
    axplat::sel4::handler(cpu_id)
}

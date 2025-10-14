use axplat_aarch64_sel4::ServiceEvent;
#[cfg(feature = "irq")]
use axplat_aarch64_sel4::irq::handle_irq;
use axplat_aarch64_sel4::task::{create_sel4_task, exit_sel4_task, migrate_sel4_task};
use axplat_aarch64_sel4::ipc::*;
use common::config::DEFAULT_SERVE_EP;
use common::{read_types, reply_with};
use sel4::{with_ipc_buffer_mut, Fault, with_ipc_buffer};

pub(crate) fn event_handler(cpu_id: usize) -> ! {
    with_ipc_buffer_mut(|ib| {
        loop {
            debug!("Waiting for message on cpu {}...", cpu_id);
            let (msg, _badge) = DEFAULT_SERVE_EP.recv(());
            #[cfg(feature = "irq")]
            if msg.label() == 0 {
                // handle interrupt
                debug!("irq number is :{}", _badge);
                handle_irq(_badge as _);
                continue;
            }
            let msg_label = match ServiceEvent::try_from(msg.label()) {
                Ok(x) => x,
                Err(_) => {
                    if msg.label() >= 8 {
                        if msg.label() == 0x204 {
                            break;
                        } else {
                            error!("Unknown root messaage label: {:#x}", msg.label());
                        }
                    }
                    let fault = with_ipc_buffer(|buffer| Fault::new(buffer, &msg));
                    error!("Received {} Fault: {:#x?}", _badge, fault);
                    continue;
                }
            };
            match msg_label {
                ServiceEvent::SwitchTask => {
                    let task_ptr = read_types!(ib, usize);
                    reply_with!(ib, 0);
                    debug!("Switch to task {:#x}", task_ptr);
                    axtask::switch_sel4_task(task_ptr);
                }
                ServiceEvent::CreateTask => {
                    let (tid, entry, stack, tls, affinity) =
                        read_types!(ib, usize, usize, usize, usize, usize);
                    info!("Create task {} entry {:#x} stack {:#x} tls {:#x} affinity {:#x} on cpu {}",
                        tid, entry, stack, tls, affinity, cpu_id);
                    if cpu_id == 0 {
                        let task_ptr = create_sel4_task(tid, entry, stack, tls, affinity);
                        reply_with!(ib, task_ptr);
                    } else {
                        let task_ptr = create_task(tid, entry, stack, tls, affinity);
                        reply_with!(ib, task_ptr);
                    }
                }
                ServiceEvent::ExitTask => {
                    let task_ptr = read_types!(ib, usize);
                    info!("Exit task {:#x} on cpu {}", task_ptr, cpu_id);
                    if cpu_id == 0 {
                        exit_sel4_task(task_ptr);
                    } else {
                        exit_task(task_ptr);
                    }
                    reply_with!(ib, 0);
                }
                ServiceEvent::ExitSystem => {
                    reply_with!(ib, 0);
                    break;
                }
                ServiceEvent::MigrateTask => {
                    let (task_ptr, target) = read_types!(ib, usize, usize);
                    if cpu_id == 0 {
                        migrate_sel4_task(task_ptr, target);
                    } else {
                        error!("Only cpu 0 can migrate tasks");
                    }
                    reply_with!(ib, 0);
                }
            }
        }
    });

    info!("Exit system");
    axhal::power::system_off();
}

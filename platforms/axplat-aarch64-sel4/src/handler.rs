#[cfg(feature = "irq")]
use crate::irq::handle_irq;
use crate::task::{create_sel4_task, exit_sel4_task, migrate_sel4_task, switch_sel4_task};
use common::config::DEFAULT_SERVE_EP;
use common::{read_types, reply_with};
use kit::ipc::ServiceEvent;
use sel4::with_ipc_buffer_mut;

use log::*;

pub(crate) fn event_handler(cpu_id: usize) -> ! {
    with_ipc_buffer_mut(|ib| {
        loop {
            let (msg, _badge) = DEFAULT_SERVE_EP.recv(());
            let msg_label = match ServiceEvent::try_from(msg.label()) {
                Ok(x) => x,
                Err(_) => {
                    if msg.label() == 0x204 {
                        break;
                    // } else if msg.label() >= 8 {
                    //     error!(
                    //         "Unknown root messaage label: {}, badge {}",
                    //         msg.label(),
                    //         _badge
                    //     );
                    //     let fault = with_ipc_buffer(|buffer| Fault::new(buffer, &msg));
                    //     error!("Received {} Fault: {:#x?}", _badge, fault);
                    //     continue;
                    } else {
                        #[cfg(feature = "irq")]
                        {
                            // log::info!("handle irq badge {} on cpu {}", _badge, cpu_id);
                            handle_irq(_badge as _);
                        }
                        continue;
                    }
                }
            };
            match msg_label {
                ServiceEvent::SwitchTask => {
                    let (prev_task, next_task) = read_types!(ib, usize, usize);
                    reply_with!(ib, 0);
                    debug!("Switch to task {:#x} on cpu {}", next_task, cpu_id);
                    switch_sel4_task(prev_task, next_task);
                }
                ServiceEvent::CreateTask => {
                    let (tid, entry, stack, tls, affinity) =
                        read_types!(ib, usize, usize, usize, usize, usize);
                    debug!(
                        "Create task {} entry {:#x} stack {:#x} tls {:#x} affinity {:#x} on cpu {}",
                        tid, entry, stack, tls, affinity, cpu_id
                    );
                    let task_ptr = create_sel4_task(tid, entry, stack, tls, affinity);
                    reply_with!(ib, task_ptr);
                }
                ServiceEvent::ExitTask => {
                    let task_ptr = read_types!(ib, usize);
                    debug!("Exit task {:#x} on cpu {}", task_ptr, cpu_id);
                    reply_with!(ib, 0);
                    exit_sel4_task(task_ptr);
                }
                ServiceEvent::ExitSystem => {
                    debug!("Exit system on cpu {}", cpu_id);
                    reply_with!(ib, 0);
                    break;
                }
                ServiceEvent::MigrateTask => {
                    let (task_ptr, target) = read_types!(ib, usize, usize);
                    debug!("migrate task on cpu {}", cpu_id);
                    reply_with!(ib, 0);
                    migrate_sel4_task(task_ptr, target);
                }
            }
        }
    });

    info!("Exit system");
    common::root::shutdown()
}

use axplat::sel4::Sel4EventIf;

struct Sel4EventIfImpl;

#[impl_plat_interface]
impl Sel4EventIf for Sel4EventIfImpl {
    fn handler(cpu_id: usize) -> ! {
        event_handler(cpu_id)
    }
}

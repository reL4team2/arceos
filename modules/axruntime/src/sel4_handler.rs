use axplat_aarch64_sel4::ServiceEvent;
#[cfg(feature = "irq")]
use axplat_aarch64_sel4::irq::handle_irq;
use axplat_aarch64_sel4::task::{create_sel4_task, exit_sel4_task};
use common::config::DEFAULT_SERVE_EP;
use common::{read_types, reply_with};
use sel4::{with_ipc_buffer_mut, Fault, with_ipc_buffer};

pub(crate) fn event_handler() -> ! {
    with_ipc_buffer_mut(|ib| {
        loop {
            info!("Waiting for message...");
            let (msg, _badge) = DEFAULT_SERVE_EP.recv(());
            #[cfg(feature = "irq")]
            if msg.label() == 0 {
                // handle interrupt
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
                    axtask::switch_sel4_task(task_ptr);
                }
                ServiceEvent::CreateTask => {
                    let (tid, entry, stack, tls, affinity) =
                        read_types!(ib, usize, usize, usize, usize, usize);
                    let task_ptr = create_sel4_task(tid, entry, stack, tls, affinity);
                    reply_with!(ib, task_ptr);
                }
                ServiceEvent::ExitTask => {
                    let task_ptr = read_types!(ib, usize);
                    exit_sel4_task(task_ptr);
                    reply_with!(ib, 0);
                }
                ServiceEvent::ExitSystem => {
                    reply_with!(ib, 0);
                    break;
                }
            }
        }
    });

    info!("Exit system");
    axhal::power::system_off();
}

#[cfg(feature = "smp")]
pub(crate) fn secondary_event_handler() -> ! {
    // only handle interrupts in secondary event handler. for timer and ipi interrupt
    with_ipc_buffer_mut(|ib| {
        loop {
            let (msg, _badge) = DEFAULT_SERVE_EP.recv(());
        }
    });

    panic!()
}

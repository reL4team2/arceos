use common_macros::generate_ipc_send;
use num_enum::{IntoPrimitive, TryFromPrimitive};

#[derive(Debug, IntoPrimitive, TryFromPrimitive)]
#[repr(u64)]
pub enum ServiceEvent {
    CreateTask = 0x1000,
    SwitchTask,
    ExitTask,
    ExitSystem,
}

macro_rules! call_ep {
    ($msg:expr) => {
        common::config::DEFAULT_PARENT_EP.call($msg)
    };
}

#[generate_ipc_send(label = ServiceEvent::CreateTask)]
pub fn create_task(tid: usize, entry: usize, kstack: usize, tls: usize) -> usize {}

#[generate_ipc_send(label = ServiceEvent::SwitchTask)]
pub fn switch_task(task: usize) -> usize {}

#[generate_ipc_send(label = ServiceEvent::ExitTask)]
pub fn exit_task(task: usize) -> usize {}

#[generate_ipc_send(label = ServiceEvent::ExitSystem)]
pub fn exit_system() -> usize {}

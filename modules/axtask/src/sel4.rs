use sel4_if::{
    create_task, destroy_task, is_init_task, migrate_task, start_task, stop_task, switch_task,
};
use sel4_oskit::ipc::{
    create_task as ipc_create_task, exit_system, exit_task, migrate_task as ipc_migrate_task,
    switch_task as ipc_switch_task,
};

pub(crate) fn sel4_create_task(
    tid: usize,
    entry: usize,
    kstack: usize,
    tls: usize,
    affinity: usize,
) -> usize {
    if is_init_task() {
        create_task(tid, entry, kstack, tls, affinity)
    } else {
        ipc_create_task(tid, entry, kstack, tls, affinity)
    }
}

pub(crate) fn sel4_switch_task(prev_task: usize, next_task: usize) -> usize {
    if is_init_task() {
        switch_task(prev_task, next_task)
    } else {
        ipc_switch_task(prev_task, next_task)
    }
}

pub(crate) fn sel4_exit_task(task: usize) {
    if is_init_task() {
        destroy_task(task);
    } else {
        exit_task(task);
    }
}

pub(crate) fn sel4_migrate_task(task: usize, cpu_id: usize) {
    if is_init_task() {
        migrate_task(task, cpu_id);
    } else {
        ipc_migrate_task(task, cpu_id);
    }
}

pub(crate) fn sel4_start_task(task: usize) {
    start_task(task)
}

#[allow(unused)]
pub(crate) fn sel4_stop_task(task: usize) {
    stop_task(task)
}

pub(crate) fn sel4_exit_system() -> usize {
    exit_system()
}

#![no_std]
#![feature(thread_local)]

#[macro_use]
extern crate axplat;

extern crate alloc;
extern crate uart_thread;

mod console;
mod init;
#[cfg(feature = "irq")]
pub mod irq;
mod mem;
mod power;
mod time;

pub mod utils;
pub use utils::task;

pub mod ipc;
pub use ipc::create_task;
pub use ipc::switch_task;
pub use ipc::exit_task;
pub use ipc::exit_system;

pub mod asm;

pub mod config {
    //! Platform configuration module.
    //!
    //! If the `AX_CONFIG_PATH` environment variable is set, it will load the configuration from the specified path.
    //! Otherwise, it will fall back to the `axconfig.toml` file in the current directory and generate the default configuration.
    //!
    //! If the `PACKAGE` field in the configuration does not match the package name, it will panic with an error message.
    axconfig_macros::include_configs!(path_env = "AX_CONFIG_PATH", fallback = "axconfig.toml");
    assert_str_eq!(
        PACKAGE,
        env!("CARGO_PKG_NAME"),
        "`PACKAGE` field in the configuration does not match the Package name. Please check your configuration file."
    );
}

#[unsafe(no_mangle)]
unsafe extern "C" fn _start() -> ! {
    axplat::call_main(0, 0);
}

pub fn migrate_task(task: usize, cpu_id: usize) -> usize {
    if crate::utils::task::init_task() {
        crate::utils::task::migrate_sel4_task(task, cpu_id);
        return 0
    } else {
        crate::ipc::migrate_task(task, cpu_id)
    }
}

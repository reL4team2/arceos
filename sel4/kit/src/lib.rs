#![no_std]
#![feature(thread_local)]
#![feature(used_with_arg)]

extern crate alloc;

pub mod asm;
pub mod config;
pub mod ipc;
#[cfg(feature = "irq")]
pub mod irq;
pub mod mem;
pub mod obj;

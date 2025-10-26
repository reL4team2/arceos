#![no_std]
#![feature(thread_local)]

extern crate alloc;

pub mod config;
pub mod ipc;
pub mod irq;
pub mod mem;
pub mod obj;

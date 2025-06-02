unsafe extern "C" {
    fn rust_main(cpu_id: usize, dtb: usize);
    #[cfg(feature = "smp")]
    fn rust_main_secondary(cpu_id: usize);
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.boot")]
pub(crate) unsafe extern "C" fn _start() {
    crate::mem::clear_bss();
    crate::cpu::init_primary(0);
    super::pl011::init_early();
    rust_main(0, 0);
}

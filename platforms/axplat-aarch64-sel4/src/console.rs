//! PL011 UART.

use arm_pl011::Pl011Uart;
use axplat::mem::VirtAddr;
use kspin::SpinNoIrq;
use lazyinit::LazyInit;
use axplat::console::ConsoleIf;

static UART: LazyInit<SpinNoIrq<Pl011Uart>> = LazyInit::new();

fn do_putchar(uart: &mut Pl011Uart, c: u8) {
    match c {
        b'\n' => {
            uart.putchar(b'\r');
            uart.putchar(b'\n');
        }
        c => uart.putchar(c),
    }
}

/// Reads a byte from the console, or returns [`None`] if no input is available.
pub fn getchar() -> Option<u8> {
    UART.lock().getchar()
}

/// Early stage initialization of the PL011 UART driver.
pub fn init_early(uart_base: VirtAddr) {
    UART.init_once(SpinNoIrq::new(Pl011Uart::new(uart_base.as_mut_ptr())));
    UART.lock().init();
}

struct ConsoleIfImpl;

#[impl_plat_interface]
impl ConsoleIf for ConsoleIfImpl {
    /// Writes given bytes to the console.
    fn write_bytes(bytes: &[u8]) {
        let mut uart = UART.lock();
        for c in bytes {
            do_putchar(&mut uart, *c);
        }
    }

    /// Reads bytes from the console into the given mutable slice.
    ///
    /// Returns the number of bytes read.
    fn read_bytes(bytes: &mut [u8]) -> usize {
        let mut read_len = 0;
        while read_len < bytes.len() {
            if let Some(c) = getchar() {
                bytes[read_len] = c;
            } else {
                break;
            }
            read_len += 1;
        }
        read_len
    }
}

//! Stable version of `panic-abort` for the Cortex-M architecture

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(warnings)]
#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    cortex_m::asm::udf()
}

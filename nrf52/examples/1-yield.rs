//! Yielding from a task
//!
//! Expected output:
//!
//! ```
//! B: yield
//! A: yield
//! B: yield
//! A: yield
//! DONE
//! ```

#![deny(unsafe_code)]
#![deny(warnings)]
#![no_main]
#![no_std]

use async_cortex_m::task;
use cortex_m::asm;
use cortex_m_rt::entry;
use cortex_m_semihosting::hprintln;
use nrf52 as _; // memory layout
use panic_udf as _; // panic handler

#[entry]
fn main() -> ! {
    // task A
    task::spawn(async {
        loop {
            hprintln!("A: yield").ok();
            // context switch to B
            task::r#yield().await;
        }
    });

    // task B
    task::block_on(async {
        hprintln!("B: yield").ok();

        // context switch to A
        task::r#yield().await;

        hprintln!("B: yield").ok();

        task::r#yield().await;

        hprintln!("DONE").ok();

        loop {
            asm::bkpt();
        }
    })
}

//! Message passing between tasks using a MPMC channel
//!
//! Expected output:
//!
//! ```
//! B: before recv
//! A: before send
//! A: after send
//! A: yield
//! B: 42
//! DONE
//! ```

#![deny(unsafe_code)]
#![deny(warnings)]
#![no_main]
#![no_std]

use async_embedded::{task, unsync::Channel};
use cortex_m::asm;
use cortex_m_rt::entry;
use cortex_m_semihosting::hprintln;
use nrf52 as _; // memory layout
use panic_udf as _; // panic handler

#[entry]
fn main() -> ! {
    static mut C: Channel<i32> = Channel::new();

    // coerce to a shared (`&-`) reference to avoid _one_ of the `move` blocks taking ownership of
    // the owning static (`&'static mut`) reference
    let c: &'static _ = C;

    task::spawn(async move {
        hprintln!("A: before send").ok();

        c.send(42).await;

        hprintln!("A: after send").ok();

        loop {
            hprintln!("A: yield").ok();
            task::r#yield().await;
        }
    });

    task::block_on(async move {
        hprintln!("B: before recv").ok();

        // cannot immediately make progress; context switch to A
        let msg = c.recv().await;

        hprintln!("B: {}", msg).ok();

        hprintln!("DONE").ok();

        loop {
            asm::bkpt();
        }
    })
}

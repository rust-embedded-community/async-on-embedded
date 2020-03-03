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

use core::time::Duration;

use async_cortex_m::task;
use cortex_m_rt::entry;
use nrf52::{led::Red, timer::Timer};
use panic_udf as _; // panic handler

#[entry]
fn main() -> ! {
    let mut timer = Timer::take();

    let dur = Duration::from_millis(100);
    task::block_on(async {
        loop {
            Red.on();
            timer.wait(dur).await;
            Red.off();
            timer.wait(dur).await;
            Red.on();
            timer.wait(dur).await;
            Red.off();
            timer.wait(12 * dur).await;
        }
    })
}

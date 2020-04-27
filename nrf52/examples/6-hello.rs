//! Spam "Hello, world!" over the serial line (@ 9600 bauds)
//!
//! TXD = P0.06
//! RXD = P0.08

#![deny(unsafe_code)]
#![deny(warnings)]
#![no_main]
#![no_std]

use core::time::Duration;

use async_embedded::task;
use cortex_m_rt::entry;
use nrf52::{led::Red, serial, timer::Timer};
use panic_udf as _; // panic handler

#[entry]
fn main() -> ! {
    // heartbeat task
    let mut timer = Timer::take();
    let dur = Duration::from_millis(100);
    task::spawn(async move {
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
    });

    let (mut tx, _rx) = serial::take();
    task::block_on(async {
        loop {
            tx.write(b"Hello, world!\n\r").await;
        }
    })
}

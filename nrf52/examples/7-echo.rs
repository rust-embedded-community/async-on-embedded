//! Echo back data received over the serial line (@ 9600 bauds)
//!
//! TXD = P0.06
//! RXD = P0.08

#![deny(unsafe_code)]
#![deny(warnings)]
#![no_main]
#![no_std]

use core::time::Duration;

use async_cortex_m::task;
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

    let (mut tx, mut rx) = serial::take();
    task::block_on(async {
        let mut buf = [0; 16];
        loop {
            rx.read(&mut buf).await;
            tx.write(&buf).await;
        }
    })
}

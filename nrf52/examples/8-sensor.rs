//! Print sensor data on demand

#![deny(unsafe_code)]
#![deny(warnings)]
#![no_main]
#![no_std]

use core::{cell::Cell, fmt::Write as _, time::Duration};

use async_embedded::{task, unsync::Mutex};
use cortex_m_rt::entry;
use heapless::{consts, String};
use nrf52::{led::Red, scd30::Scd30, serial, timer::Timer, twim::Twim};
use panic_udf as _; // panic handler

#[derive(Clone, Copy)]
enum State {
    NotReady,
    Ready,
    Error,
}

#[entry]
fn main() -> ! {
    // shared state
    static mut STATE: Cell<State> = Cell::new(State::NotReady);
    // range: 0 - 40,000 ppm
    static mut CO2: Cell<u16> = Cell::new(0);
    // range: 0 - 100 %
    static mut RH: Cell<u8> = Cell::new(0);
    // range: -40 - 70 C
    static mut T: Cell<i8> = Cell::new(0);
    static mut M: Option<Mutex<Twim>> = None;

    let co2: &'static _ = CO2;
    let state: &'static _ = STATE;
    let rh: &'static _ = RH;
    let t: &'static _ = T;

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

    // task to print sensor info on demand
    let (mut tx, mut rx) = serial::take();
    task::spawn(async move {
        let mut tx_buf = String::<consts::U32>::new();
        let mut rx_buf = [0];

        loop {
            rx.read(&mut rx_buf).await;

            // carriage return;
            if rx_buf[0] == 13 {
                match state.get() {
                    State::Error => {
                        tx.write(b"fatal error: I2C error\n").await;

                        loop {
                            task::r#yield().await;
                        }
                    }

                    State::NotReady => {
                        tx.write(b"sensor not ready; try again later\n").await;
                    }

                    State::Ready => {
                        let co2 = co2.get();
                        let t = t.get();
                        let rh = rh.get();

                        tx_buf.clear();
                        // will not fail; the buffer is big enough
                        let _ = writeln!(&mut tx_buf, "CO2: {}ppm\nT: {}C\nRH: {}%", co2, t, rh);
                        tx.write(tx_buf.as_bytes()).await;
                    }
                }
            }
        }
    });

    // task to continuously poll the sensor
    let twim = M.get_or_insert(Mutex::new(Twim::take()));
    let mut scd30 = Scd30::new(twim);
    task::block_on(async {
        loop {
            if let Ok(m) = scd30.get_measurement().await {
                co2.set(m.co2 as u16);
                rh.set(m.rh as u8);
                t.set(m.t as i8);
                state.set(State::Ready);
            } else {
                state.set(State::Error);

                loop {
                    task::r#yield().await;
                }
            }
        }
    })
}

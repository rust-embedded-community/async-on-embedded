//! Interactive serial console with access to the clock and gas sensor
//!
//! Example interaction (with local echo enabled):
//!
//! ```
//! > help
//! Commands:
//! help              displays this text
//! date              display the current date and time
//! sensors           displays the gas sensor data
//! set date %Y-%m-%d changes the date
//! set time %H:%M:%S changes the time
//! > sensors
//! CO2: 652ppm
//! T: 26C
//! RH: 23%
//! > set time 18:49:30
//! > date
//! 2020-02-28 18:49:32
//! ```

#![deny(unsafe_code)]
#![deny(warnings)]
#![no_main]
#![no_std]

use core::{
    cell::Cell,
    fmt::Write as _,
    str::{self, FromStr},
    time::Duration,
};

use async_cortex_m::{task, unsync::Mutex};
use chrono::{Datelike as _, NaiveDate, NaiveTime};
use cortex_m_rt::entry;
use heapless::{consts, String, Vec};
use nrf52::{
    ds3231::{self, Ds3231},
    led::Red,
    scd30::Scd30,
    serial,
    timer::Timer,
    twim::Twim,
};
use panic_udf as _; // panic handler

#[derive(Clone, Copy)]
enum SensorState {
    NotReady,
    Ready,
    Error,
}

#[entry]
fn main() -> ! {
    // shared state
    static mut STATE: Cell<SensorState> = Cell::new(SensorState::NotReady);
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

    let twim = M.get_or_insert(Mutex::new(Twim::take()));
    let mut scd30 = Scd30::new(twim);
    task::spawn(async move {
        loop {
            // TODO instead of continuously polling the sensor we should only read it out when new
            // data is ready (there's a pin that signals that), roughly every 2 seconds.
            // Alternatively, we could send this task to sleep for 2 seconds after new data is read
            let res = scd30.get_measurement().await;

            if let Ok(m) = res {
                co2.set(m.co2 as u16);
                rh.set(m.rh as u8);
                t.set(m.t as i8);
                state.set(SensorState::Ready);

                // adds fairness; avoids starving the task below
                task::r#yield().await;
            } else {
                state.set(SensorState::Error);

                loop {
                    task::r#yield().await;
                }
            }
        }
    });

    let (mut tx, mut rx) = serial::take();
    let mut ds3231 = Ds3231::new(twim);
    task::block_on(async {
        let mut input = Vec::<u8, consts::U64>::new();
        let mut tx_buf = String::<consts::U32>::new();

        'prompt: loop {
            tx.write(b"> ").await;

            input.clear();
            loop {
                // not the most elegant way to have a responsive input
                // Ideally, we want to use a large `rx_buf` and instead read its contents only when
                // there has been no new data on the bus for a while
                let mut rx_buf = [0];
                rx.read(&mut rx_buf).await;

                if input.push(rx_buf[0]).is_err() {
                    tx.write(b"input buffer is full\n").await;
                    continue 'prompt;
                }

                if let Ok(s) = str::from_utf8(&input) {
                    // complete command
                    if s.ends_with('\r') {
                        if let Ok(cmd) = s[..s.len() - 1].parse::<Command>() {
                            match cmd {
                                Command::Date => {
                                    match ds3231.get_datetime().await {
                                        Ok(datetime) => {
                                            let mut s = String::<consts::U32>::new();
                                            // will not fail; the buffer is big enough
                                            let _ = writeln!(&mut s, "{}", datetime);
                                            tx.write(s.as_bytes()).await;
                                        }

                                        Err(ds3231::Error::Twim(..)) => {
                                            tx.write(b"error communicating with the RTC\n").await;
                                        }

                                        Err(ds3231::Error::InvalidDate) => {
                                            tx.write(b"invalid date stored in the RTC\n").await;
                                        }
                                    }
                                }

                                Command::SetDate(date) => {
                                    // in `Command::parse_str` we validate the input date so no
                                    // `InvalidDate` error should be raised here
                                    if ds3231.set_date(date).await.is_err() {
                                        tx.write(b"error communicating with the RTC\n").await;
                                    }
                                }

                                Command::SetTime(time) => {
                                    if ds3231.set_time(time).await.is_err() {
                                        tx.write(b"error communicating with the RTC\n").await;
                                    }
                                }

                                Command::Sensors => {
                                    tx_buf.clear();

                                    // will not fail; the buffer is big enough
                                    let _ = writeln!(
                                        &mut tx_buf,
                                        "CO2: {}ppm\nT: {}C\nRH: {}%",
                                        co2.get(),
                                        t.get(),
                                        rh.get()
                                    );
                                    tx.write(tx_buf.as_bytes()).await;
                                }

                                Command::Help => {
                                    tx.write(
                                        b"Commands:
help              displays this text
date              display the current date and time
sensors           displays the gas sensor data
set date %Y-%m-%d changes the date
set time %H:%M:%S changes the time
",
                                    )
                                    .await;
                                }
                            }
                        } else {
                            tx.write(b"invalid command; try `help`\n").await;
                        }

                        // new prompt; clear command buffer
                        continue 'prompt;
                    }
                }
            }
        }
    })
}

enum Command {
    Date,
    Help,
    Sensors,
    SetDate(NaiveDate),
    SetTime(NaiveTime),
}

impl FromStr for Command {
    type Err = Error;

    fn from_str(mut s: &str) -> Result<Command, Error> {
        const CMD_DATE: &str = "date";
        const CMD_HELP: &str = "help";
        const CMD_SENSORS: &str = "sensors";
        const CMD_SET_DATE: &str = "set date ";
        const CMD_SET_TIME: &str = "set time ";

        s = s.trim();

        Ok(if s == CMD_DATE {
            Command::Date
        } else if s == CMD_HELP {
            Command::Help
        } else if s == CMD_SENSORS {
            Command::Sensors
        } else if s.starts_with(CMD_SET_DATE) {
            let date = NaiveDate::parse_from_str(&s[CMD_SET_DATE.len()..], "%Y-%m-%d")
                .map_err(|_| Error)?;
            let year = date.year();
            // the RTC can only handle a span of roughly 200 years
            if year < 2000 || year > 2199 {
                return Err(Error);
            }

            Command::SetDate(date)
        } else if s.starts_with(CMD_SET_TIME) {
            let time = NaiveTime::parse_from_str(&s[CMD_SET_TIME.len()..], "%H:%M:%S")
                .map_err(|_| Error)?;

            Command::SetTime(time)
        } else {
            return Err(Error);
        })
    }
}

struct Error;

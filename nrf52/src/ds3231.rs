//! Asynchronous DS3231 (Real-Time Clock) driver

// Reference: DS3231 datasheet (19-5170; Rev 10; 3/15)

use async_cortex_m::unsync::Mutex;
use chrono::{Datelike as _, NaiveDate, NaiveDateTime, NaiveTime, Timelike as _};

use crate::twim::{self, Twim};

const ADDRESS: u8 = 0b110_1000;

// Address map
const SECONDS: u8 = 0;
const DATE: u8 = 4;

/// DS3231 I2C driver
pub struct Ds3231<'a> {
    twim: &'a Mutex<Twim>,
}

// 12-hour format (AM / PM)
const HOUR12: u8 = 1 << 6;
// PM half
const PM: u8 = 1 << 5;

const CENTURY: u8 = 1 << 7;

/// Driver error
#[derive(Debug)]
pub enum Error {
    /// The RTC cannot hold this date
    InvalidDate,

    /// I2C error
    Twim(twim::Error),
}

impl From<twim::Error> for Error {
    fn from(e: twim::Error) -> Error {
        Error::Twim(e)
    }
}

impl<'a> Ds3231<'a> {
    /// Creates a new driver
    pub fn new(twim: &'a Mutex<Twim>) -> Self {
        Self { twim }
    }

    /// Returns the current date
    pub async fn get_date(&mut self) -> Result<NaiveDate, Error> {
        let mut buf = [0; 3];
        self.twim
            .lock()
            .await
            .write_then_read(ADDRESS, &[DATE], &mut buf)
            .await?;

        date_from_regs(&buf)
    }

    /// Returns the current date and time
    pub async fn get_datetime(&mut self) -> Result<NaiveDateTime, Error> {
        let mut buf = [0; 7];
        self.twim
            .lock()
            .await
            .write_then_read(ADDRESS, &[SECONDS], &mut buf)
            .await?;

        let time = time_from_regs(&buf[..3]);
        let date = date_from_regs(&buf[4..])?;

        Ok(date.and_time(time))
    }

    /// Returns the current time
    pub async fn get_time(&mut self) -> Result<NaiveTime, twim::Error> {
        let mut buf = [0; 3];
        self.twim
            .lock()
            .await
            .write_then_read(ADDRESS, &[SECONDS], &mut buf)
            .await?;

        Ok(time_from_regs(&buf))
    }

    /// Changes the current date
    pub async fn set_date(&mut self, date: NaiveDate) -> Result<(), Error> {
        let day = to_bcd(date.day() as u8);
        let mut month = to_bcd(date.month() as u8);
        let mut year = date.year();
        if year < 2000 || year > 2199 {
            return Err(Error::InvalidDate);
        }
        year -= 2000;
        if year >= 100 {
            month |= CENTURY;
            year -= 100;
        }
        let year = to_bcd(year as u8);

        self.twim
            .lock()
            .await
            .write(ADDRESS, &[DATE, day, month, year])
            .await?;
        Ok(())
    }

    /// Changes the current time
    pub async fn set_time(&mut self, time: NaiveTime) -> Result<(), twim::Error> {
        let sec = to_bcd(time.second() as u8);
        let min = to_bcd(time.minute() as u8);
        let hour = to_bcd(time.hour() as u8);

        self.twim
            .lock()
            .await
            .write(ADDRESS, &[SECONDS, sec, min, hour])
            .await
    }
}

fn time_from_regs(regs: &[u8]) -> NaiveTime {
    let sec = from_bcd(regs[0]);
    let min = from_bcd(regs[1]);
    let hour = if regs[2] & HOUR12 != 0 {
        if regs[2] & PM != 0 {
            from_bcd(12 + (regs[2] & !PM))
        } else {
            from_bcd(regs[2])
        }
    } else {
        // 24-hour format
        from_bcd(regs[2])
    };

    NaiveTime::from_hms(hour.into(), min.into(), sec.into())
}

fn date_from_regs(regs: &[u8]) -> Result<NaiveDate, Error> {
    let day = from_bcd(regs[0]);
    let month = from_bcd(regs[1] & !CENTURY);
    let year = i32::from(from_bcd(regs[2])) + if regs[1] & CENTURY != 0 { 2100 } else { 2000 };

    NaiveDate::from_ymd_opt(year, month.into(), day.into()).ok_or(Error::InvalidDate)
}

fn from_bcd(bcd: u8) -> u8 {
    let units = bcd & 0b1111;
    let tens = bcd >> 4;

    10 * tens + units
}

#[allow(dead_code)]
fn to_bcd(x: u8) -> u8 {
    let units = x % 10;
    let tens = x / 10;
    tens << 4 | units
}

//! Asynchronous SCD30 (gas sensor) driver

// Reference: Interface Description Sensirion SCD30 Sensor Module (Version
// 0.94–D1 –June 2019)

use async_embedded::unsync::Mutex;

use crate::twim::{self, Twim};

/// Sensor measurement
#[derive(Clone, Copy)]
pub struct Measurement {
    /// CO2 concentrain in parts per million (0 - 40,000 ppm)
    pub co2: f32,

    /// Relative humidity (0 - 100%)
    pub rh: f32,

    /// Temperature in Celsius (-40 - 70 C)
    pub t: f32,
}

const ADDRESS: u8 = 0x61;

/// SCD30 I2C driver
pub struct Scd30<'a> {
    twim: &'a Mutex<Twim>,
}

/// Driver error
#[derive(Debug)]
pub enum Error {
    /// Checksum error
    Checksum,

    /// I2C error
    Twim(twim::Error),
}

impl From<twim::Error> for Error {
    fn from(e: twim::Error) -> Self {
        Error::Twim(e)
    }
}

impl<'a> Scd30<'a> {
    /// Creates a new driver
    pub fn new(twim: &'a Mutex<Twim>) -> Self {
        Self { twim }
    }

    /// Returns the last sensor measurement
    pub async fn get_measurement(&mut self) -> Result<Measurement, Error> {
        while !self.data_ready().await? {
            continue;
        }

        let mut buf = [0; 18];
        {
            let mut twim = self.twim.lock().await;
            twim.write(ADDRESS, &[0x03, 0x00]).await?;
            twim.read(ADDRESS, &mut buf).await?;
            drop(twim);
        }

        for chunk in buf.chunks(3) {
            if !crc_check(&chunk[..2], chunk[2]) {
                return Err(Error::Checksum);
            }
        }

        let co2 = f32::from_le_bytes([buf[4], buf[3], buf[1], buf[0]]);
        let t = f32::from_le_bytes([buf[10], buf[9], buf[7], buf[6]]);
        let rh = f32::from_le_bytes([buf[16], buf[15], buf[13], buf[12]]);

        Ok(Measurement { co2, t, rh })
    }

    async fn data_ready(&mut self) -> Result<bool, Error> {
        let mut buf = [0; 3];
        {
            let mut twim = self.twim.lock().await;
            twim.write(ADDRESS, &[0x02, 0x02]).await?;
            twim.read(ADDRESS, &mut buf).await?;
            drop(twim);
        }

        if !crc_check(&buf[..2], buf[2]) {
            return Err(Error::Checksum);
        }

        Ok(buf[1] == 1)
    }
}

// TODO
fn crc_check(_bytes: &[u8], _crc: u8) -> bool {
    true
}

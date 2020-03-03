//! LEDs

// NOTE(borrow_unchecked) all writes are single-instruction, atomic operations
// on a stateless register

use pac::P0;

use crate::BorrowUnchecked as _;

// NOTE called from `pre_init`
pub(crate) fn init() {
    pac::P0::borrow_unchecked(|p0| {
        // set outputs lows
        p0.outset
            .write(|w| w.pin13().set_bit().pin14().set_bit().pin15().set_bit());

        // set pins as output
        p0.dirset
            .write(|w| w.pin13().set_bit().pin14().set_bit().pin15().set_bit());
    });
}

/// Red LED
pub struct Red;

impl Red {
    /// Turns the LED off
    pub fn off(&self) {
        P0::borrow_unchecked(|p0| p0.outset.write(|w| w.pin13().set_bit()))
    }

    /// Turns the LED on
    pub fn on(&self) {
        P0::borrow_unchecked(|p0| p0.outclr.write(|w| w.pin13().set_bit()))
    }
}

/// Green LED
pub struct Green;

impl Green {
    /// Turns the LED off
    pub fn off(&self) {
        P0::borrow_unchecked(|p0| p0.outset.write(|w| w.pin14().set_bit()))
    }

    /// Turns the LED on
    pub fn on(&self) {
        P0::borrow_unchecked(|p0| p0.outclr.write(|w| w.pin14().set_bit()))
    }
}

/// Blue LED
pub struct Blue;

impl Blue {
    /// Turns the LED off
    pub fn off(&self) {
        P0::borrow_unchecked(|p0| p0.outset.write(|w| w.pin15().set_bit()))
    }

    /// Turns the LED on
    pub fn on(&self) {
        P0::borrow_unchecked(|p0| p0.outclr.write(|w| w.pin15().set_bit()))
    }
}

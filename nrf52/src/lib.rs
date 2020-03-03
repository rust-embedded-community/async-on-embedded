//! Asynchronous HAL for the nRF52840

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(warnings)]
#![no_std]

use core::{marker::PhantomData, mem};

use cortex_m_rt::pre_init;

pub mod ds3231;
pub mod led;
pub mod scd30;
pub mod serial;
pub mod timer;
pub mod twim;

pub use timer::Timer;

// peripheral initialization
#[pre_init]
unsafe fn pre_init() {
    // configure the LFCLK to use the external crystal (32.768Hz)
    pac::CLOCK::borrow_unchecked(|clock| {
        clock.lfclksrc.write(|w| w.src().xtal());
        clock
            .tasks_lfclkstart
            .write(|w| w.tasks_lfclkstart().set_bit());
        while clock
            .events_lfclkstarted
            .read()
            .events_lfclkstarted()
            .bit_is_clear()
        {
            // busy wait
            continue;
        }
    });

    // LEDs
    led::init();

    // Serial port
    serial::init();

    // TWIM
    twim::init();

    // start the RTC
    timer::init();

    // sadly we cannot seal the configuration of the peripherals from this
    // context (static variables are uninitialized at this point)
    // drop(pac::Peripheral::take());
}

/// Borrows a peripheral without checking if it has already been taken
unsafe trait BorrowUnchecked {
    fn borrow_unchecked<T>(f: impl FnOnce(&Self) -> T) -> T;
}

macro_rules! borrow_unchecked {
    ($($peripheral:ident),*) => {
        $(
            unsafe impl BorrowUnchecked for pac::$peripheral {
                fn borrow_unchecked<T>(f: impl FnOnce(&Self) -> T) -> T {
                    let p = unsafe { mem::transmute(()) };
                    f(&p)
                }
            }
        )*
    }
}

borrow_unchecked!(CLOCK, P0, RTC0, TWIM0, UARTE0);

struct NotSync {
    _inner: PhantomData<*mut ()>,
}

impl NotSync {
    fn new() -> Self {
        Self {
            _inner: PhantomData,
        }
    }
}

unsafe impl Send for NotSync {}

fn slice_in_ram(slice: &[u8]) -> bool {
    const RAM_START: usize = 0x2000_0000;
    const RAM_SIZE: usize = 128 * 1024;
    const RAM_END: usize = RAM_START + RAM_SIZE;

    let start = slice.as_ptr() as usize;
    let end = start + slice.len();

    RAM_START <= start && end < RAM_END
}

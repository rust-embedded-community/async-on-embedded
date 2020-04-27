//! Sharing state between tasks using `Cell` and `RefCell`
//!
//! ```
//! B: initialize x
//! B: post a message through y
//! B: yield
//! A: x=42
//! A: received a message through y: 42
//! A: yield
//! DONE
//! ```

#![deny(unsafe_code)]
#![deny(warnings)]
#![no_main]
#![no_std]

use core::cell::{Cell, RefCell};

use async_embedded::task;
use cortex_m::asm;
use cortex_m_rt::entry;
use cortex_m_semihosting::hprintln;
use nrf52 as _; // memory layout
use panic_udf as _; // panic handler

#[entry]
fn main() -> ! {
    static mut X: Cell<i64> = Cell::new(0);
    // not-async-aware, one-shot channel
    static mut Y: RefCell<Option<i32>> = RefCell::new(None);

    // only references with `'static` lifetimes can be sent to `spawn`-ed tasks
    // NOTE we coerce these to a shared (`&-`) reference to avoid the `move` blocks taking ownership
    // of the owning static (`&'static mut`) reference
    let x: &'static Cell<_> = X;
    let y: &'static RefCell<_> = Y;

    // task A
    task::spawn(async move {
        hprintln!("A: x={}", x.get()).ok();

        if let Some(msg) = y.borrow_mut().take() {
            hprintln!("A: received a message through y: {}", msg).ok();
        }

        loop {
            hprintln!("A: yield").ok();
            // context switch to B
            task::r#yield().await;
        }
    });

    // task B
    task::block_on(async {
        hprintln!("B: initialize x").ok();
        x.set(42);

        hprintln!("B: post a message through y").ok();
        *y.borrow_mut() = Some(42);

        hprintln!("B: yield").ok();

        // context switch to A
        task::r#yield().await;

        hprintln!("DONE").ok();

        loop {
            asm::bkpt();
        }
    })
}

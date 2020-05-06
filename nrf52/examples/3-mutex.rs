//! Mutex shared between tasks
//!
//! "When to use `Mutex` instead of a `RefCell`?" Both abstractions will give you an exclusive
//! (`&mut-`) reference to the data and that reference can survive across `yield`s (either explicit
//! , i.e. `task::yield`, or implicit, `.await`).
//!
//! The difference between the two is clear when contention occurs. If two or more tasks contend for
//! a `RefCell`, as in they both call `borrow_mut` on it, you'll get a panic. On the other hand, if
//! you use a `Mutex` in a similar scenario, i.e. both tasks call `lock` on it, then one of them
//! will "asynchronous" wait for (i.e. not resume until) the other task to release (releases) the
//! lock.
//!
//! Expected output:
//!
//! ```
//! B: before lock
//! A: before write
//! A: after releasing the lock
//! A: yield
//! B: 42
//! DONE
//! ```
//!
//! Try to replace the `Mutex` with `RefCell` and re-run the example

#![deny(unsafe_code)]
#![deny(warnings)]
#![no_main]
#![no_std]

use async_cortex_m::{task, unsync::Mutex};
use cortex_m::asm;
use cortex_m_rt::entry;
use cortex_m_semihosting::hprintln;
use nrf52 as _; // memory layout
use panic_udf as _; // panic handler

#[entry]
fn main() -> ! {
    static mut X: Mutex<i64> = Mutex::new(0);

    let mut lock = X.try_lock().unwrap();

    task::spawn(async {
        hprintln!("A: before write").ok();
        *lock = 42;
        drop(lock);

        hprintln!("A: after releasing the lock").ok();

        loop {
            hprintln!("A: yield").ok();
            task::r#yield().await;
        }
    });

    task::block_on(async {
        hprintln!("B: before lock").ok();

        // cannot immediately make progress; context switch to A
        let lock = X.lock().await;

        hprintln!("B: {}", *lock).ok();

        hprintln!("DONE").ok();

        loop {
            asm::bkpt();
        }
    })
}

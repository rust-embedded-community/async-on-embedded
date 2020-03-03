//! Timers

use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{self, AtomicBool, Ordering},
    task::{Context, Poll, Waker},
    time::Duration,
};

use cortex_m::peripheral::NVIC;
use pac::{Interrupt, RTC0};

use crate::{BorrowUnchecked as _, NotSync};

// NOTE called from `pre_init`
pub(crate) fn init() {
    pac::RTC0::borrow_unchecked(|rtc| {
        // enable compare0 interrupt
        rtc.intenset.write(|w| w.compare0().set_bit());
        rtc.tasks_clear.write(|w| w.tasks_clear().set_bit());
        rtc.tasks_start.write(|w| w.tasks_start().set_bit());
    });
}

/// [singleton] An `async`-aware timer
pub struct Timer {
    _not_sync: NotSync,
}

impl Timer {
    /// Takes the singleton instance of this timer
    ///
    /// This returns the `Some` variant only once
    pub fn take() -> Self {
        // NOTE peripheral initialization is done in `#[pre_init]`

        static TAKEN: AtomicBool = AtomicBool::new(false);

        if TAKEN
            .compare_exchange_weak(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            Self {
                _not_sync: NotSync::new(),
            }
        } else {
            panic!("`Timer` has already been taken")
        }
    }

    /// Waits for at least `dur`
    // NOTE we could support several "timeouts" by making this take `&self` and
    // using a priority queue (sorted queue) to store the deadlines
    pub async fn wait(&mut self, dur: Duration) {
        struct Wait<'a> {
            _timer: &'a mut Timer,
            installed_waker: bool,
        }

        impl<'a> Future for Wait<'a> {
            type Output = ();

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
                static mut WAKER: Option<Waker> = None;

                if has_expired() {
                    if self.installed_waker {
                        // uninstall the waker
                        NVIC::mask(Interrupt::RTC0);
                        // NOTE(compiler_fence) the interrupt must be disabled
                        // before we take down the waker
                        atomic::compiler_fence(Ordering::SeqCst);
                        drop(unsafe { WAKER.take() })
                    }

                    Poll::Ready(())
                } else {
                    if !self.installed_waker {
                        unsafe {
                            WAKER = Some(cx.waker().clone());
                            // NOTE(compiler_fence) `WAKER` write must complete
                            // before we enable the interrupt
                            atomic::compiler_fence(Ordering::Release);
                            NVIC::unmask(Interrupt::RTC0); // atomic write
                        }

                        #[allow(non_snake_case)]
                        #[no_mangle]
                        fn RTC0() {
                            // NOTE(unsafe) the only other context that can
                            // access this static variable runs at lower
                            // priority -- that context won't overlap in
                            // execution with this operation
                            if let Some(waker) = unsafe { WAKER.as_ref() } {
                                waker.wake_by_ref();

                                // one shot interrupt -- this won't fire again
                                NVIC::mask(Interrupt::RTC0);
                            } else {
                                // this could be have been triggered by the user
                            }
                        }
                    } else {
                        // prepare another one-shot interrupt
                        unsafe {
                            NVIC::unmask(Interrupt::RTC0);
                        }
                    }

                    Poll::Pending
                }
            }
        }

        // TODO do this without 64-bit arithmetic
        const F: u64 = 32_768; // frequency of the LFCLK
        let ticks = dur.as_secs() * F + (u64::from(dur.subsec_nanos()) * F) / 1_000_000_000;
        // NOTE we could support 64-bit ticks
        assert!(ticks < (1 << 24));
        let ticks = ticks as u32;

        NVIC::mask(Interrupt::RTC0);
        RTC0::borrow_unchecked(|rtc| {
            let now = rtc.counter.read().bits();
            rtc.events_compare[0].reset();
            // NOTE(unsafe) this operation shouldn't be marked as `unsafe`
            rtc.cc[0].write(|w| unsafe { w.compare().bits(now.wrapping_add(ticks)) });
        });

        Wait {
            _timer: self,
            installed_waker: false,
        }
        .await
    }
}

fn has_expired() -> bool {
    RTC0::borrow_unchecked(|rtc| {
        if rtc.events_compare[0].read().events_compare().bit_is_set() {
            rtc.events_compare[0].reset();
            true
        } else {
            false
        }
    })
}

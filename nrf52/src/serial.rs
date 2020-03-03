//! Serial interface

// Based on https://github.com/nrf-rs/nrf52-hal/commit/f05d471996c63f605cab43aa76c8fd990b852460

use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{self, AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};

use cortex_m::peripheral::NVIC;
use pac::{Interrupt, UARTE0};

use crate::{BorrowUnchecked as _, NotSync};

// NOTE called from `pre_init`
pub(crate) fn init() {
    use pac::uarte0::baudrate::BAUDRATE_A;

    pac::UARTE0::borrow_unchecked(|uarte| {
        const TX_PIN: u8 = 6;
        const RX_PIN: u8 = 8;
        const UARTE_PORT: bool = false; // 0

        // Select pins
        uarte.psel.rxd.write(|w| unsafe {
            w.pin()
                .bits(RX_PIN)
                .port()
                .bit(UARTE_PORT)
                .connect()
                .connected()
        });
        // pins.txd.set_high().unwrap();
        uarte.psel.txd.write(|w| unsafe {
            w.pin()
                .bits(TX_PIN)
                .port()
                .bit(UARTE_PORT)
                .connect()
                .connected()
        });

        // Enable UARTE instance
        uarte.enable.write(|w| w.enable().enabled());

        // enable interrupts
        uarte
            .intenset
            .write(|w| w.endtx().set_bit().endrx().set_bit());

        // Configure frequency
        uarte
            .baudrate
            .write(|w| w.baudrate().variant(BAUDRATE_A::BAUD9600));
    });
}

const INTERRUPT: Interrupt = Interrupt::UARTE0_UART0;

/// Takes the singleton instance of the serial interface
///
/// The interface is split in transmitter and receiver parts
///
/// This returns the `Some` variant only once
pub fn take() -> (Tx, Rx) {
    // NOTE peripheral initialization is done in `#[pre_init]`

    static TAKEN: AtomicBool = AtomicBool::new(false);

    if TAKEN
        .compare_exchange_weak(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        (            Tx {
                _not_sync: NotSync::new(),
            },
            Rx {
                _not_sync: NotSync::new(),
            },
)
    } else {
        panic!("serial device has already been taken");
    }
}

/// [Singleton] Receiver component of the serial interface
pub struct Rx {
    _not_sync: NotSync,
}

impl Rx {
    /// *Completely* fills the given `buffer` with bytes received over the serial interface
    // XXX(Soundness?) The following operation is potentially unsound: `buf`
    // points into RAM; the future returned by this method is `poll`-ed once and
    // then `mem::forget`-ed (forgotten). This lets the caller return from the
    // current stack frame, freeing `buf`: now the DMA can overwrite the stack
    // frames of the program
    // TODO bubble up errors
    pub async fn read(&mut self, buf: &mut [u8]) {
        struct Read<'t, 'b> {
            _rx: &'t mut Rx,
            buf: &'b mut [u8],
            state: State,
        }

        impl Future for Read<'_, '_> {
            type Output = ();

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
                match self.state {
                    // nothing to do
                    State::NotStarted if self.buf.len() == 0 => {
                        self.state = State::Finished;

                        Poll::Ready(())
                    }

                    State::NotStarted => {
                        UARTE0::borrow_unchecked(|uarte| {
                            // reset events
                            uarte.events_endrx.reset();

                            uarte
                                .rxd
                                .maxcnt
                                .write(|w| unsafe { w.maxcnt().bits(self.buf.len() as u16) });

                            uarte.rxd.ptr.write(|w| unsafe {
                                w.ptr().bits(self.buf.as_mut_ptr() as usize as u32)
                            });

                            // install the waker
                            NVIC::mask(INTERRUPT);
                            unsafe {
                                RX_WAKER = Some(cx.waker().clone());
                                // NOTE(compiler_fence) writing the waker must
                                // complete before the interrupt is unmasked
                                atomic::compiler_fence(Ordering::Release);
                                NVIC::unmask(INTERRUPT);
                            }

                            // start the transfer
                            // semantically this complete the transfer of the
                            // reference to the DMA; any pending write to
                            // `bytes` must complete before the transfer, hence
                            // the compiler fence -- but it's redundant because
                            // of the preceding barrier
                            atomic::compiler_fence(Ordering::Release);
                            uarte.tasks_startrx.write(|w| unsafe { w.bits(1) });
                        });

                        self.state = State::InProgress;

                        Poll::Pending
                    }

                    State::InProgress => {
                        UARTE0::borrow_unchecked(|uarte| {
                            if uarte.events_endrx.read().bits() != 0 {
                                uarte.events_endrx.reset();

                                self.state = State::Finished;

                                // uninstall the waker
                                NVIC::mask(INTERRUPT);
                                // NOTE(compiler_fence) the interrupt must be
                                // disabled before we take down the waker
                                atomic::compiler_fence(Ordering::SeqCst);
                                drop(unsafe { RX_WAKER.take() });
                                unsafe {
                                    // the TX waker may still need to be serviced
                                    if TX_WAKER.is_some() {
                                        NVIC::unmask(INTERRUPT);
                                    }
                                }

                                Poll::Ready(())
                            } else {
                                // spurious wake up; re-arm the one-shot interrupt
                                unsafe {
                                    NVIC::unmask(INTERRUPT);
                                }

                                Poll::Pending
                            }
                        })
                    }

                    State::Finished => unreachable!(),
                }
            }
        }

        impl Drop for Read<'_, '_> {
            fn drop(&mut self) {
                if self.state == State::InProgress {
                    // stop the transfer
                    todo!()
                }
            }
        }

        // TODO for large buffers do transfers in chunks
        assert!(buf.len() < (1 << 10));

        Read {
            _rx: self,
            buf,
            state: State::NotStarted,
        }
        .await
    }
}

/// [Singleton] Receiver component of the serial interface
pub struct Tx {
    _not_sync: NotSync,
}

impl Tx {
    /// Sends *all* `bytes` over the serial interface
    // NOTE like with `read`, starting a `write` on a `bytes` that points into
    // the stack, `poll`-ing the future and then `mem::forget`-ing it is a Bad
    // Thing To Do. This operation is not unsound on the device side but will
    // sent junk through the serial interface
    // TODO bubble up errors
    pub async fn write(&mut self, bytes: &[u8]) {
        if crate::slice_in_ram(bytes) {
            self.write_from_ram(bytes).await
        } else {
            const BUFSZ: usize = 128;
            let mut on_the_stack = [0; BUFSZ];
            for chunk in bytes.chunks(BUFSZ) {
                let n = chunk.len();
                on_the_stack[..n].copy_from_slice(chunk);
                self.write_from_ram(&on_the_stack[..n]).await
            }
        }
    }

    // `bytes` has already been checked to point into RAM
    async fn write_from_ram(&mut self, bytes: &[u8]) {
        struct Write<'t, 'b> {
            _tx: &'t mut Tx,
            bytes: &'b [u8],
            state: State,
        }

        impl Future for Write<'_, '_> {
            type Output = ();

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
                match self.state {
                    // nothing to do
                    State::NotStarted if self.bytes.len() == 0 => {
                        self.state = State::Finished;

                        Poll::Ready(())
                    }

                    State::NotStarted => {
                        UARTE0::borrow_unchecked(|uarte| {
                            // reset events
                            uarte.events_endtx.reset();

                            uarte
                                .txd
                                .maxcnt
                                .write(|w| unsafe { w.maxcnt().bits(self.bytes.len() as u16) });

                            uarte.txd.ptr.write(|w| unsafe {
                                w.ptr().bits(self.bytes.as_ptr() as usize as u32)
                            });

                            // install the waker
                            NVIC::mask(INTERRUPT);
                            unsafe {
                                TX_WAKER = Some(cx.waker().clone());
                                // NOTE(compiler_fence) writing the waker must
                                // complete before the interrupt is unmasked
                                atomic::compiler_fence(Ordering::Release);
                                NVIC::unmask(INTERRUPT);
                            }

                            // start the transfer
                            // semantically this complete the transfer of the
                            // reference to the DMA; any pending write to
                            // `bytes` must complete before the transfer, hence
                            // the compiler fence -- but it's redundant because
                            // of the preceding barrier
                            atomic::compiler_fence(Ordering::Release);
                            uarte.tasks_starttx.write(|w| unsafe { w.bits(1) });
                        });

                        self.state = State::InProgress;

                        Poll::Pending
                    }

                    State::InProgress => {
                        UARTE0::borrow_unchecked(|uarte| {
                            if uarte.events_endtx.read().bits() != 0 {
                                uarte.events_endtx.reset();

                                self.state = State::Finished;

                                // uninstall the waker
                                NVIC::mask(INTERRUPT);
                                // NOTE(compiler_fence) the interrupt must be
                                // disabled before we take down the waker
                                atomic::compiler_fence(Ordering::SeqCst);
                                drop(unsafe { TX_WAKER.take() });
                                unsafe {
                                    // the RX waker may still need to be serviced
                                    if RX_WAKER.is_some() {
                                        NVIC::unmask(INTERRUPT);
                                    }
                                }

                                Poll::Ready(())
                            } else {
                                // spurious wake up; re-arm the one-shot interrupt
                                unsafe {
                                    NVIC::unmask(INTERRUPT);
                                }

                                Poll::Pending
                            }
                        })
                    }

                    State::Finished => unreachable!(),
                }
            }
        }

        impl Drop for Write<'_, '_> {
            fn drop(&mut self) {
                if self.state == State::InProgress {
                    // stop the transfer
                    todo!()
                }
            }
        }

        // TODO for large buffers do transfers in chunks
        assert!(bytes.len() < (1 << 10));

        Write {
            _tx: self,
            bytes,
            state: State::NotStarted,
        }
        .await
    }
}

static mut RX_WAKER: Option<Waker> = None;
static mut TX_WAKER: Option<Waker> = None;

#[allow(non_snake_case)]
#[no_mangle]
fn UARTE0_UART0() {
    let mut ran_a_waker = false;
    unsafe {
        if let Some(waker) = RX_WAKER.as_ref() {
            waker.wake_by_ref();
            ran_a_waker = true;
        }

        if let Some(waker) = TX_WAKER.as_ref() {
            waker.wake_by_ref();
            ran_a_waker = true;
        }
    }

    if ran_a_waker {
        // avoid continuously re-entering this interrupt handler
        NVIC::mask(INTERRUPT);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    NotStarted,
    InProgress,
    Finished,
}

//! Two-Wire Interface (AKA I2C)

// Based on https://github.com/nrf-rs/nrf52-hal/commit/f05d471996c63f605cab43aa76c8fd990b852460

use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{self, AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};

use cortex_m::peripheral::NVIC;
use pac::{Interrupt, TWIM0};

use crate::{BorrowUnchecked, NotSync};

// NOTE called from `pre_init`
pub(crate) fn init() {
    use pac::twim0::frequency::FREQUENCY_A;

    const SDA_PIN: u8 = 26;
    const SCL_PIN: u8 = 27;
    const TWIM_PORT: bool = false; // 0

    // pin configuration
    pac::P0::borrow_unchecked(|p0| {
        for pin in [SDA_PIN, SCL_PIN].iter() {
            p0.pin_cnf[*pin as usize].write(|w| {
                w.dir()
                    .input()
                    .input()
                    .connect()
                    .pull()
                    .pullup()
                    .drive()
                    .s0d1()
                    .sense()
                    .disabled()
            });
        }
    });

    pac::TWIM0::borrow_unchecked(|twim| {
        twim.psel.scl.write(|w| unsafe {
            w.pin()
                .bits(SCL_PIN)
                .port()
                .bit(TWIM_PORT)
                .connect()
                .connected()
        });

        twim.psel.sda.write(|w| unsafe {
            w.pin()
                .bits(SDA_PIN)
                .port()
                .bit(TWIM_PORT)
                .connect()
                .connected()
        });

        // Enable the TWIM interface
        twim.enable.write(|w| w.enable().enabled());

        // Configure frequency
        twim.frequency
            .write(|w| w.frequency().variant(FREQUENCY_A::K100));

        twim.intenset
            .write(|w| w.error().set_bit().stopped().set_bit());
    });
}

const INTERRUPT: Interrupt = Interrupt::SPIM0_SPIS0_TWIM0_TWIS0_SPI0_TWI0;
const MAXCNT: usize = 256;

/// [singleton] An `async`-aware I2C host
pub struct Twim {
    _not_sync: NotSync,
}

impl Twim {
    /// Takes the singleton instance of this I2C bus
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
            panic!("`Twim` has already been taken")
        }
    }

    /// Fills the given buffer with data from the device with the specified address
    ///
    /// Events: START - ADDR - (D -> H) - STOP
    ///
    /// `(D -> H)` denotes data being sent from the Device to the Host
    pub async fn read(&mut self, address: u8, buf: &mut [u8]) -> Result<(), Error> {
        struct Read<'t, 'b> {
            _twim: &'t mut Twim,
            address: u8,
            buf: &'b mut [u8],
            state: State,
        }

        impl Future for Read<'_, '_> {
            type Output = Result<(), Error>;

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
                match self.state {
                    State::NotStarted => {
                        TWIM0::borrow_unchecked(|twim| {
                            NVIC::mask(INTERRUPT);

                            // NOTE program defensively: the user could poll a `Read` future once
                            // (and start the DMA transfer) and then `mem::forget` (or `drop`) it.
                            // We cannot assume any `async` method was driven to completion
                            if twim.events_rxstarted.read().bits() != 0
                                || twim.events_txstarted.read().bits() != 0
                            {
                                // abort any pending transaction
                                twim.tasks_stop.write(|w| unsafe { w.bits(1) });

                                // clear any unhandled error
                                twim.errorsrc.reset();

                                // clear any unhandled event
                                twim.events_error.reset();
                                twim.events_lastrx.reset();
                                twim.events_lasttx.reset();
                                twim.events_stopped.reset();
                            }

                            // NOTE(unsafe) this operation is not unsafe at all
                            twim.address
                                .write(|w| unsafe { w.address().bits(self.address) });

                            twim.rxd
                                .ptr
                                .write(|w| unsafe { w.ptr().bits(self.buf.as_mut_ptr() as u32) });
                            twim.rxd
                                .maxcnt
                                .write(|w| unsafe { w.maxcnt().bits(self.buf.len() as u16) });

                            // send STOP after last byte is transmitted
                            twim.shorts.write(|w| w.lastrx_stop().set_bit());

                            // here we finishing transferring the slice to the
                            // DMA; all previous memory operations on the slice
                            // should be finished before then, thus the compiler
                            // fence
                            atomic::compiler_fence(Ordering::Release);
                            twim.tasks_startrx.write(|w| unsafe { w.bits(1) });

                            // install the waker
                            unsafe {
                                WAKER = Some(cx.waker().clone());

                                // updating the `WAKER` needs to be completed before unmasking the
                                // interrupt; hence the compiler fence
                                atomic::compiler_fence(Ordering::Release);
                                NVIC::unmask(INTERRUPT);
                            }

                            self.state = State::InProgress;

                            Poll::Pending
                        })
                    }

                    State::InProgress => {
                        TWIM0::borrow_unchecked(|twim| {
                            if twim.events_error.read().bits() != 0 {
                                // slice has been handed back to us; any future operation on the
                                // slice should not be reordered to before this point
                                atomic::compiler_fence(Ordering::Acquire);

                                // XXX do we need to clear `events_{stopped,lastrx}` here?
                                twim.events_stopped.reset();
                                twim.events_rxstarted.reset();
                                twim.events_lastrx.reset();

                                self.state = State::Finished;

                                Poll::Ready(Err(Error::Src(twim.errorsrc.read().bits() as u8)))
                            } else if twim.events_stopped.read().bits() != 0 {
                                // slice has been handed back to us; any future operation on the
                                // slice should not be reordered to before this point
                                atomic::compiler_fence(Ordering::Acquire);

                                // events have been successfully handled
                                twim.events_stopped.reset();
                                twim.events_rxstarted.reset();
                                twim.events_lastrx.reset();

                                // uninstall the waker
                                NVIC::mask(INTERRUPT);
                                // NOTE(compiler_fence) the interrupt must be
                                // disabled before we take down the waker
                                atomic::compiler_fence(Ordering::Release);
                                drop(unsafe { WAKER.take() });

                                let amount = twim.rxd.amount.read().bits() as u8;

                                self.state = State::Finished;

                                let n = self.buf.len() as u8;
                                if amount == n {
                                    Poll::Ready(Ok(()))
                                } else {
                                    Poll::Ready(Err(Error::ShortRead(amount)))
                                }
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

        // TODO do reads/writes in chunks?
        assert!(buf.len() < MAXCNT);

        Read {
            _twim: self,
            address,
            buf,
            state: State::NotStarted,
        }
        .await
    }

    /// `write` followed by `read` in a single transaction (without an intermediate STOP)
    ///
    /// Events: START - ADDR - (H -> D) - reSTART - ADDR - (D -> H) - STOP
    ///
    /// `reSTART` denotes a "repeated START"
    pub async fn write_then_read(
        &mut self,
        address: u8,
        wr_buf: &[u8],
        rd_buf: &mut [u8],
    ) -> Result<(), Error> {
        // TODO do reads/writes in chunks?
        assert!(wr_buf.len() < MAXCNT && rd_buf.len() < MAXCNT);

        if crate::slice_in_ram(wr_buf) {
            self.write_from_ram_then_read(address, wr_buf, rd_buf).await
        } else {
            let mut buf = [0; MAXCNT];
            let n = wr_buf.len();
            buf[..n].copy_from_slice(wr_buf);
            self.write_from_ram_then_read(address, &buf[..n], rd_buf)
                .await
        }
    }

    async fn write_from_ram_then_read(
        &mut self,
        address: u8,
        wr_buf: &[u8],
        rd_buf: &mut [u8],
    ) -> Result<(), Error> {
        struct WriteThenRead<'t, 'b> {
            _twim: &'t mut Twim,
            address: u8,
            rd_buf: &'b mut [u8],
            state: State,
            wr_buf: &'b [u8],
        }

        impl Future for WriteThenRead<'_, '_> {
            type Output = Result<(), Error>;

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
                match self.state {
                    State::NotStarted => {
                        TWIM0::borrow_unchecked(|twim| {
                            NVIC::mask(INTERRUPT);

                            // NOTE program defensively: the user could poll a `WriteThenRead`
                            // future once (and start the DMA transfer) and then `mem::forget` (or
                            // `drop`) it. We cannot assume any `async` method was driven to
                            // completion
                            if twim.events_rxstarted.read().bits() != 0
                                || twim.events_txstarted.read().bits() != 0
                            {
                                // abort any pending transaction
                                twim.tasks_stop.write(|w| unsafe { w.bits(1) });

                                // clear any unhandled error
                                twim.errorsrc.reset();

                                // clear any unhandled event
                                twim.events_error.reset();
                                twim.events_lastrx.reset();
                                twim.events_lasttx.reset();
                                twim.events_stopped.reset();
                            }

                            // NOTE(unsafe) this operation is not unsafe at all
                            twim.address
                                .write(|w| unsafe { w.address().bits(self.address) });

                            twim.rxd.ptr.write(|w| unsafe {
                                w.ptr().bits(self.rd_buf.as_mut_ptr() as u32)
                            });
                            twim.rxd
                                .maxcnt
                                .write(|w| unsafe { w.maxcnt().bits(self.rd_buf.len() as u16) });

                            twim.txd
                                .ptr
                                .write(|w| unsafe { w.ptr().bits(self.wr_buf.as_ptr() as u32) });
                            twim.txd
                                .maxcnt
                                .write(|w| unsafe { w.maxcnt().bits(self.wr_buf.len() as u16) });

                            // start read after write is finished and trigger a
                            // STOP after the read is finished
                            twim.shorts
                                .write(|w| w.lasttx_startrx().set_bit().lastrx_stop().set_bit());

                            // here we finishing transferring the slices to the
                            // DMA; all previous memory operations on the slices
                            // should be finished before then, thus the compiler fence
                            atomic::compiler_fence(Ordering::Release);
                            twim.tasks_starttx.write(|w| unsafe { w.bits(1) });

                            // install the waker
                            unsafe {
                                WAKER = Some(cx.waker().clone());

                                // updating the `WAKER` needs to be done before
                                // unmasking the interrupt; hence the compiler fence
                                atomic::compiler_fence(Ordering::Release);
                                NVIC::unmask(INTERRUPT);
                            }

                            self.state = State::InProgress;

                            Poll::Pending
                        })
                    }

                    State::InProgress => {
                        TWIM0::borrow_unchecked(|twim| {
                            if twim.events_error.read().bits() != 0 {
                                // slice has been handed back to us; any future operation on the
                                // slice should not be reordered to before this point
                                atomic::compiler_fence(Ordering::Acquire);

                                // XXX do we need to clear `events_{stopped,lastrx,lasttx}` here?
                                twim.events_stopped.reset();
                                twim.events_rxstarted.reset();
                                twim.events_lastrx.reset();
                                twim.events_txstarted.reset();
                                twim.events_lasttx.reset();

                                self.state = State::Finished;

                                Poll::Ready(Err(Error::Src(twim.errorsrc.read().bits() as u8)))
                            } else if twim.events_stopped.read().bits() != 0 {
                                // slice has been handed back to us; any future operation on the
                                // slice should not be reordered to before this point
                                atomic::compiler_fence(Ordering::Acquire);

                                // events have been successfully handled
                                twim.events_stopped.reset();
                                twim.events_rxstarted.reset();
                                twim.events_lastrx.reset();
                                twim.events_txstarted.reset();
                                twim.events_lasttx.reset();

                                // uninstall the waker
                                NVIC::mask(INTERRUPT);
                                // NOTE(compiler_fence) the interrupt must be
                                // disabled before we take down the waker
                                atomic::compiler_fence(Ordering::Release);
                                drop(unsafe { WAKER.take() });

                                let amount = twim.rxd.amount.read().bits() as u8;

                                if amount != self.rd_buf.len() as u8 {
                                    return Poll::Ready(Err(Error::ShortRead(amount)));
                                }

                                let amount = twim.txd.amount.read().bits() as u8;
                                if amount != self.wr_buf.len() as u8 {
                                    return Poll::Ready(Err(Error::ShortWrite(amount)));
                                }

                                self.state = State::Finished;

                                Poll::Ready(Ok(()))
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

        impl Drop for WriteThenRead<'_, '_> {
            fn drop(&mut self) {
                if self.state == State::InProgress {
                    // stop the transfer
                    todo!()
                }
            }
        }

        WriteThenRead {
            _twim: self,
            address,
            rd_buf,
            state: State::NotStarted,
            wr_buf,
        }
        .await
    }

    /// Sends `bytes` to the device with the specified address
    ///
    /// Events: START - ADDR - (H -> D) - STOP
    ///
    /// `(H -> D)` denotes data being sent from the Host to the Device
    pub async fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Error> {
        // TODO do writes in chunks?
        assert!(bytes.len() < MAXCNT);

        if crate::slice_in_ram(bytes) {
            self.write_from_ram(address, bytes).await
        } else {
            let mut buf = [0; MAXCNT];
            let n = bytes.len();
            buf[..n].copy_from_slice(bytes);
            self.write_from_ram(address, &buf[..n]).await
        }
    }

    // NOTE `bytes` points into RAM
    async fn write_from_ram(&mut self, address: u8, bytes: &[u8]) -> Result<(), Error> {
        struct Write<'t, 'b> {
            _twim: &'t Twim,
            address: u8,
            bytes: &'b [u8],
            state: State,
        }

        impl Future for Write<'_, '_> {
            type Output = Result<(), Error>;

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
                match self.state {
                    State::NotStarted => {
                        TWIM0::borrow_unchecked(|twim| {
                            NVIC::mask(INTERRUPT);

                            // NOTE program defensively: the user could poll a `Write` future (start
                            // the transfer) and then `mem::forget` it. We cannot assume any `async`
                            // method was driven to completion
                            if twim.events_rxstarted.read().bits() != 0
                                || twim.events_txstarted.read().bits() != 0
                            {
                                // abort any pending transaction
                                twim.tasks_stop.write(|w| unsafe { w.bits(1) });

                                // clear any unhandled error
                                twim.errorsrc.reset();

                                // clear any unhandled event
                                twim.events_error.reset();
                                twim.events_lastrx.reset();
                                twim.events_lasttx.reset();
                                twim.events_stopped.reset();
                            }

                            // NOTE(unsafe) this operation is not unsafe at all
                            twim.address
                                .write(|w| unsafe { w.address().bits(self.address) });

                            twim.txd
                                .ptr
                                .write(|w| unsafe { w.ptr().bits(self.bytes.as_ptr() as u32) });
                            twim.txd
                                .maxcnt
                                .write(|w| unsafe { w.maxcnt().bits(self.bytes.len() as u16) });

                            // send STOP after last byte is transmitted
                            twim.shorts.write(|w| w.lasttx_stop().set_bit());

                            // here we finishing transferring the slice to the DMA; all previous
                            // memory operations on the slice should be finished before then, thus
                            // the compiler fence
                            atomic::compiler_fence(Ordering::Release);
                            twim.tasks_starttx.write(|w| unsafe { w.bits(1) });

                            // install the waker
                            unsafe {
                                WAKER = Some(cx.waker().clone());

                                // updating the `WAKER` needs to be complete before unmasking the
                                // interrupt; hence the compiler fence
                                atomic::compiler_fence(Ordering::Release);
                                NVIC::unmask(INTERRUPT);
                            }

                            self.state = State::InProgress;

                            Poll::Pending
                        })
                    }

                    State::InProgress => {
                        TWIM0::borrow_unchecked(|twim| {
                            if twim.events_error.read().bits() != 0 {
                                // slice has been handed back to us; any future operation on the
                                // slice should not be reordered to before this point
                                atomic::compiler_fence(Ordering::Acquire);

                                // XXX do we need to clear `events_{stopped,lasttx}` here?
                                twim.events_stopped.reset();
                                twim.events_txstarted.reset();
                                twim.events_lasttx.reset();

                                self.state = State::Finished;

                                Poll::Ready(Err(Error::Src(twim.errorsrc.read().bits() as u8)))
                            } else if twim.events_stopped.read().bits() != 0 {
                                // slice has been handed back to us; any future operation on the
                                // slice should not be reordered to before this point
                                atomic::compiler_fence(Ordering::Acquire);

                                // events have been successfully handled
                                twim.events_stopped.reset();
                                twim.events_txstarted.reset();
                                twim.events_lasttx.reset();

                                // uninstall the waker
                                NVIC::mask(INTERRUPT);
                                // NOTE(compiler_fence) the interrupt must be
                                // disabled before we take down the waker
                                atomic::compiler_fence(Ordering::Release);
                                drop(unsafe { WAKER.take() });

                                let amount = twim.txd.amount.read().bits() as u8;

                                self.state = State::Finished;

                                let n = self.bytes.len() as u8;
                                if amount == n {
                                    Poll::Ready(Ok(()))
                                } else {
                                    Poll::Ready(Err(Error::ShortWrite(amount)))
                                }
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

        Write {
            _twim: self,
            address,
            bytes,
            state: State::NotStarted,
        }
        .await
    }
}

static mut WAKER: Option<Waker> = None;

#[allow(non_snake_case)]
#[no_mangle]
fn SPIM0_SPIS0_TWIM0_TWIS0_SPI0_TWI0() {
    // NOTE(unsafe) the only other context that can access this static variable
    // runs at lower priority
    if let Some(waker) = unsafe { WAKER.as_ref() } {
        waker.wake_by_ref();

        // avoid continuously re-entering this interrupt handler
        NVIC::mask(INTERRUPT);
    } else {
        // reachable if the user manually pends this interrupt
    }
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    NotStarted,
    InProgress,
    Finished,
}

/// I2C error
#[derive(Debug)]
pub enum Error {
    /// Wrote less data than requested
    ShortWrite(u8),

    /// Read less data than requested
    ShortRead(u8),

    /// ERRORSRC encoded error
    Src(u8),
}

// NOTE waker logic is based on async-std v1.5.0

// A `!Sync` MPMC queue / Channel is just a classic ring buffer that consists of
// an array with read and write cursors

use core::{
    cell::{Cell, UnsafeCell},
    future::Future,
    marker::Unpin,
    mem::MaybeUninit,
    pin::Pin,
    task::{Context, Poll},
};

use generic_array::{typenum::Unsigned, GenericArray};

use super::waker_set::WakerSet;

/// MPMC channel
// FIXME this needs a destructor
// TODO make this generic over the capacity -- that would require the newtype with public field hack
// to keep the `const-fn` `new`. See `heapless` for examples of the workaround
// TODO a SPSC version of this. It should not need the `WakerSet` but rather something like
// `Option<Waker>`
pub struct Channel<T> {
    buffer: UnsafeCell<MaybeUninit<GenericArray<T, crate::NTASKS>>>,
    read: Cell<usize>,
    write: Cell<usize>,
    send_wakers: WakerSet,
    recv_wakers: WakerSet,
}

impl<T> Channel<T> {
    /// Creates a new fixed capacity channel
    pub const fn new() -> Self {
        Self {
            buffer: UnsafeCell::new(MaybeUninit::uninit()),
            read: Cell::new(0),
            write: Cell::new(0),
            send_wakers: WakerSet::new(),
            recv_wakers: WakerSet::new(),
        }
    }

    /// Sends a message into the channel
    pub async fn send(&self, val: T) {
        struct Send<'a, T> {
            channel: &'a Channel<T>,
            msg: Option<T>,
            opt_key: Option<usize>,
        }

        // XXX(japaric) why is this required here but not in `Recv`? is it due
        // to `msg.take()`?
        impl<T> Unpin for Send<'_, T> {}

        impl<T> Future for Send<'_, T> {
            type Output = ();

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
                let msg = self.msg.take().expect("UNREACHABLE");

                // If the current task is in the set, remove it.
                if let Some(key) = self.opt_key.take() {
                    self.channel.send_wakers.remove(key);
                }

                if let Err(msg) = self.channel.try_send(msg) {
                    self.msg = Some(msg);

                    // Insert this send operation.
                    self.opt_key = Some(self.channel.send_wakers.insert(cx));

                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            }
        }

        Send {
            channel: self,
            msg: Some(val),
            opt_key: None,
        }
        .await
    }

    /// Receives a message from the channel
    pub async fn recv(&self) -> T {
        struct Recv<'a, T> {
            channel: &'a Channel<T>,
            opt_key: Option<usize>,
        }

        impl<T> Future for Recv<'_, T> {
            type Output = T;

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
                // If the current task is in the set, remove it.
                if let Some(key) = self.opt_key.take() {
                    self.channel.recv_wakers.remove(key);
                }

                // Try receiving a message.
                if let Some(msg) = self.channel.try_recv() {
                    Poll::Ready(msg)
                } else {
                    // Insert this receive operation.
                    self.opt_key = Some(self.channel.recv_wakers.insert(cx));
                    Poll::Pending
                }
            }
        }

        Recv {
            channel: self,
            opt_key: None,
        }
        .await
    }

    /// Attempts to receive a message from the channel
    ///
    /// Returns None if the channel is currently empty
    pub fn try_recv(&self) -> Option<T> {
        unsafe {
            let read = self.read.get();
            let write = self.write.get();
            let cap = crate::NTASKS::USIZE;
            let bufferp = self.buffer.get() as *mut T;

            if write > read {
                let cursor = read % cap;
                let val = bufferp.add(cursor).read();
                self.read.set(read.wrapping_add(1));
                // notify a sender
                self.send_wakers.notify_one();
                crate::signal_event_ready();
                Some(val)
            } else {
                // empty
                None
            }
        }
    }

    /// Attempts to send a message into the channel
    ///
    /// Returns an error if the channel buffer is currently full
    pub fn try_send(&self, val: T) -> Result<(), T> {
        unsafe {
            let read = self.read.get();
            let write = self.write.get();
            let cap = crate::NTASKS::USIZE;
            let bufferp = self.buffer.get() as *mut T;

            if write < read + cap {
                let cursor = write % cap;
                bufferp.add(cursor).write(val);
                self.write.set(write.wrapping_add(1));
                // notify a receiver
                self.recv_wakers.notify_one();
                crate::signal_event_ready();
                Ok(())
            } else {
                // full
                Err(val)
            }
        }
    }
}

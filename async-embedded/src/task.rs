//! Asynchronous tasks

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::executor;

/// Drives the future `f` to completion
///
/// This also makes any previously `spawn`-ed future make progress
pub fn block_on<T>(f: impl Future<Output = T>) -> T {
    executor::current().block_on(f)
}

/// Spawns a task onto the executor
///
/// The spawned task will not make any progress until `block_on` is called.
///
/// The future `f` must never terminate. The program will *abort* if `f` (the async code) returns.
/// The right signature here would be `f: impl Future<Output = !>` but that requires nightly
pub fn spawn<T>(f: impl Future<Output = T> + 'static) {
    executor::current().spawn(f)
}

/// Use `r#yield.await` to suspend the execution of a task
pub async fn r#yield() {
    struct Yield {
        yielded: bool,
    }

    impl Future for Yield {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if self.yielded {
                Poll::Ready(())
            } else {
                self.yielded = true;
                // wake ourselves
                cx.waker().wake_by_ref();
                unsafe { crate::signal_event_ready(); }
                Poll::Pending
            }
        }
    }

    Yield { yielded: false }.await
}

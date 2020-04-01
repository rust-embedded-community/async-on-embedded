use core::{
    cell::{Cell, UnsafeCell},
    future::Future,
    mem::MaybeUninit,
    pin::Pin,
    sync::atomic::{self, AtomicBool, Ordering},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use heapless::Vec;
use pin_utils::pin_mut;

use crate::{alloc::Alloc, NTASKS};

/// A single-threaded executor that only works in ARM Cortex-M "Thread mode"
/// (outside of interrupt context)
///
/// This is a singleton
pub struct Executor {
    in_block_on: Cell<bool>,
    // NOTE `UnsafeCell` is used to minimize the span of references to the `Vec`
    tasks: UnsafeCell<Vec<&'static Task, NTASKS>>,
}

// NOTE `*const ()` is &AtomicBool
static VTABLE: RawWakerVTable = {
    unsafe fn clone(p: *const ()) -> RawWaker {
        RawWaker::new(p, &VTABLE)
    }
    unsafe fn wake(p: *const ()) {
        wake_by_ref(p)
    }
    unsafe fn wake_by_ref(p: *const ()) {
        (*(p as *const AtomicBool)).store(true, Ordering::Release)
    }
    unsafe fn drop(_: *const ()) {
        // no-op
    }

    RawWakerVTable::new(clone, wake, wake_by_ref, drop)
};

impl Executor {
    /// Creates a new instance of the executor
    pub fn new() -> Self {
        Self {
            in_block_on: Cell::new(false),
            tasks: UnsafeCell::new(Vec::new()),
        }
    }

    pub fn block_on<T>(&self, f: impl Future<Output = T>) -> T {
        // we want to avoid reentering `block_on` because then all the code
        // below has to become more complex. It's also likely that the
        // application will only call `block_on` once on an infinite task
        // (`Future<Output = !>`)
        if self.in_block_on.get() {
            // nested `block_on`
            crate::abort();
        }
        self.in_block_on.set(true);

        pin_mut!(f);
        let ready = AtomicBool::new(true);
        let waker =
            unsafe { Waker::from_raw(RawWaker::new(&ready as *const _ as *const _, &VTABLE)) };
        let val = loop {
            let mut task_woken = false;

            // advance the main task
            if ready.load(Ordering::Acquire) {
                task_woken = true;
                ready.store(false, Ordering::Release);

                let mut cx = Context::from_waker(&waker);
                if let Poll::Ready(val) = f.as_mut().poll(&mut cx) {
                    break val;
                }
            }

            // advance other tasks
            // NOTE iteration ought to be OK because `tasks` can't be reallocated (it's a statically
            // allocated `heapless::Vec<T>`); `tasks` can't shrink either
            let len = unsafe { (*self.tasks.get()).len() }; // (A)
            for i in 0..len {
                let task = unsafe { (*self.tasks.get()).get_unchecked(i) };

                // NOTE we don't need a CAS operation here because `wake` invocations that come from
                // interrupt handlers (the only source of 'race conditions' (!= data races)) are
                // "oneshot": they'll issue a `wake` and then disable themselves to not run again
                // until the woken task has made more work
                if task.ready.load(Ordering::Acquire) {
                    task_woken = true;

                    // we are about to service the task so switch the `ready` flag to `false`
                    task.ready.store(false, Ordering::Release);

                    // NOTE we never deallocate tasks so `&ready` is always pointing to
                    // allocated memory (`&'static AtomicBool`)
                    let waker = unsafe {
                        Waker::from_raw(RawWaker::new(&task.ready as *const _ as *const _, &VTABLE))
                    };
                    let mut cx = Context::from_waker(&waker);
                    // this points into a `static` memory so it's already pinned
                    if unsafe {
                        !Pin::new_unchecked(&mut *task.f.get())
                            .poll(&mut cx)
                            .is_ready()
                    } {
                        continue;
                    }
                }
            }

            if task_woken {
                // If at least one task was woken up, do not sleep, try again
                continue;
            }

            // try to sleep; this will be a no-op if any of the previous tasks generated a SEV or an
            // interrupt ran (regardless of whether it generated a wake-up or not)
            unsafe { crate::wait_for_event() };
        };
        self.in_block_on.set(false);
        val
    }

    // NOTE CAREFUL! this method can overlap with `block_on`
    // FIXME we want to use `Future<Output = !>` here but the never type (`!`) is unstable; so as a
    // workaround we'll "abort" if the task / future terminates (see `Task::new`)
    pub fn spawn(&self, f: impl Future + 'static) {
        // NOTE(unsafe) only safe as long as `spawn` is never re-entered and this does not overlap
        // with operation `(A)` (see `Task::block_on`)
        let res = unsafe { (*self.tasks.get()).push(Task::new(f)) };
        if res.is_err() {
            // OOM
            crate::abort()
        }
    }
}

type Task = Node<dyn Future<Output = ()> + 'static>;

pub struct Node<F>
where
    F: ?Sized,
{
    ready: AtomicBool,
    f: UnsafeCell<F>,
}

impl Task {
    fn new(f: impl Future + 'static) -> &'static mut Self {
        // NOTE(unsafe) Only safe as long as `Executor::spawn` is not re-entered
        unsafe {
            // Already initialized at this point
            let alloc = ALLOC.get() as *mut Alloc;
            (*alloc).alloc_init(Node {
                ready: AtomicBool::new(true),
                f: UnsafeCell::new(async {
                    f.await;
                    // `spawn`-ed tasks must never terminate
                    crate::abort()
                }),
            })
        }
    }
}

static mut ALLOC: UnsafeCell<MaybeUninit<Alloc>> = UnsafeCell::new(MaybeUninit::uninit());

/// Returns a handle to the executor singleton
///
/// This lazily initializes the executor and allocator when first called
pub(crate) fn current() -> &'static Executor {
    static INIT: AtomicBool = AtomicBool::new(false);
    static mut EXECUTOR: UnsafeCell<MaybeUninit<Executor>> = UnsafeCell::new(MaybeUninit::uninit());

    if !in_thread_mode() {
        // tried to access the executor from a thread that's not `main`
        crate::abort()
    }

    if INIT.load(Ordering::Relaxed) {
        unsafe { &*(EXECUTOR.get() as *const Executor) }
    } else {
        unsafe {
            /// Reserved memory for the bump allocator (TODO this could be user configurable)
            static mut MEMORY: [u8; 1024] = [0; 1024];

            let executorp = EXECUTOR.get() as *mut Executor;
            executorp.write(Executor::new());
            let allocp = ALLOC.get() as *mut Alloc;
            allocp.write(Alloc::new(&mut MEMORY));
            // force the `allocp` write to complete before returning from this function
            atomic::compiler_fence(Ordering::Release);
            INIT.store(true, Ordering::Relaxed);
            &*executorp
        }
    }
}

fn in_thread_mode() -> bool {
    const SCB_ICSR: *const u32 = 0xE000_ED04 as *const u32;
    // NOTE(unsafe) single-instruction load with no side effects
    unsafe { SCB_ICSR.read_volatile() as u8 == 0 }
}

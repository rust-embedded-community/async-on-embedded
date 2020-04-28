//! Proof of Concept async runtime for the Cortex-M architecture

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(warnings)]
#![no_std]

mod alloc;
mod executor;
pub mod task;
pub mod unsync;

#[cfg(target_arch = "arm")]
use cortex_m::asm;


#[cfg(target_arch = "arm")]
pub use cortex_m_udf::udf as abort;

#[cfg(target_arch = "arm")]
#[inline]
/// Prevent next `wait_for_interrupt` from sleeping, wake up other harts if needed.
/// This particular implementation does nothing, since `wait_for_interrupt` never sleeps
pub(crate) unsafe fn signal_event_ready() {
    asm::sev();
}

#[cfg(target_arch = "arm")]
#[inline]
/// Wait for an interrupt or until notified by other hart via `signal_task_ready`
/// This particular implementation does nothing
pub(crate) unsafe fn wait_for_event() {
    asm::wfe();
}

#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
/// This keeps dropping into the debugger and never returns
pub fn abort() -> ! {
    loop {
        unsafe { riscv::asm::ebreak() }
    }
}

#[cfg(all(any(target_arch = "riscv32", target_arch = "riscv64"), feature = "riscv-wait-nop"))]
#[inline]
/// Prevent next `wait_for_interrupt` from sleeping, wake up other harts if needed.
/// This particular implementation does nothing, since `wait_for_interrupt` never sleeps
pub(crate) unsafe fn signal_event_ready() {}

#[cfg(all(any(target_arch = "riscv32", target_arch = "riscv64"), feature = "riscv-wait-nop"))]
#[inline]
/// Wait for an interrupt or until notified by other hart via `signal_task_ready`
/// This particular implementation does nothing
pub(crate) unsafe fn wait_for_event() {}

#[cfg(all(any(target_arch = "riscv32", target_arch = "riscv64"), feature = "riscv-wait-extern"))]
extern "C" {
    /// Prevent next `wait_for_interrupt` from sleeping, wake up other harts if needed.
    /// User is expected to provide an actual implementation, like the one shown below.
    ///
    /// #[no_mangle]
    /// pub extern "C" fn signal_event_ready() {
    ///     unimplemented!();
    /// }
    pub(crate) fn signal_event_ready();

    /// Wait for an interrupt or until notified by other hart via `signal_task_ready`
    /// User is expected to provide an actual implementation, like the one shown below.
    ///
    /// #[no_mangle]
    /// pub extern "C" fn wait_for_event() {
    ///     unimplemented!();
    /// }
    pub(crate) fn wait_for_event();
}

#[cfg(all(any(target_arch = "riscv32", target_arch = "riscv64"), feature = "riscv-wait-wfi-single-hart"))]
static mut TASK_READY: bool = false;

#[cfg(all(any(target_arch = "riscv32", target_arch = "riscv64"), feature = "riscv-wait-wfi-single-hart"))]
#[inline]
/// Prevent next `wait_for_interrupt` from sleeping, wake up other harts if needed.
/// This particular implementation prevents `wait_for_interrupt` from sleeping by setting
/// a global mutable flag
pub(crate) unsafe fn signal_event_ready() {
    TASK_READY = true;
}

#[cfg(all(any(target_arch = "riscv32", target_arch = "riscv64"), feature = "riscv-wait-wfi-single-hart"))]
#[inline]
/// Wait for an interrupt or until notified by other hart via `signal_task_ready`
/// This particular implementation decides whether to sleep or not by checking
/// a global mutable flag that's set by `signal_task_ready`
pub(crate) unsafe fn wait_for_event() {
    if !TASK_READY {
        riscv::asm::wfi();
        TASK_READY = false;
    }
}

/// Maximum number of tasks (TODO this could be user configurable)
type NTASKS = typenum::consts::U8;

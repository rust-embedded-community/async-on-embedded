//! UDF instruction on stable

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(warnings)]
#![no_std]

// FIXME this should be in `cortex-m`
// a unstable version of this is available in `core::intrinsics::abort`
/// Undefined Instruction -- this will cause the HardFault handler to preempt the caller's context
pub fn udf() -> ! {
    extern "C" {
        fn __udf() -> !;
    }
    unsafe { __udf() }
}

//! Proof of Concept async runtime for the Cortex-M architecture

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(warnings)]
#![no_std]

mod alloc;
mod executor;
pub mod task;
pub mod unsync;

use cortex_m_udf::udf as abort;

/// Maximum number of tasks (TODO this could be user configurable)
type NTASKS = typenum::consts::U8;

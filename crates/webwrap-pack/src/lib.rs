//! Packaging core: turns generator inputs into specialized Windows apps.
#![forbid(unsafe_code)]

mod error;
pub mod pe;

pub use error::PackError;
pub use pe::{inspect, machine_of, specialize_raw, Arch, Inspection};

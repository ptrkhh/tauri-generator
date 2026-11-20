//! Packaging core: turns generator inputs into specialized Windows apps.
#![forbid(unsafe_code)]

mod error;
pub mod generate;
pub mod pe;
pub mod spec;

pub use error::PackError;
pub use generate::{generate, Generated, Output, Replaced, Templates};
pub use pe::{inspect, machine_of, specialize_raw, Arch, Inspection};
pub use spec::{build_spec, check_icon, derive_slug, AppInput};

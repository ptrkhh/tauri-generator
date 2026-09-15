#![forbid(unsafe_code)]

pub mod identity;

pub use identity::{app_identity, canonicalize_url, IdentityError};

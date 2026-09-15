#![forbid(unsafe_code)]

pub mod identity;
pub mod policy;

pub use identity::{app_identity, canonicalize_url, IdentityError};
pub use policy::{is_trusted_origin, nav_verdict, NavEvent, Verdict};

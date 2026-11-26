#![forbid(unsafe_code)]

pub mod identity;
pub mod policy;
pub mod schema;

pub use identity::{app_identity, canonicalize_url, IdentityError};
pub use policy::{is_trusted_origin, nav_verdict, NavEvent, Verdict};
pub use schema::{
    check_assoc, Adapter, AppSpec, Assoc, DnsMode, SpecError, MAX_SPEC_BYTES, RESERVED_DEVICE_NAMES,
};

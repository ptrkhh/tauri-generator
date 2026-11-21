//! How a file reaches a page, and what the user is told about it (D4).

use serde::Deserialize;
use webwrap_spec::Adapter;

/// What a generic page's script asks for in `webwrap-ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Want {
    /// The site set a `launchQueue` consumer: post read-write handles.
    Handle,
    /// Post shared buffers.
    Buffer,
}

/// How the runtime posts one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Handle,
    Buffer,
}

/// Which way a file was opened, from strongest to weakest claim.
///
/// A page reports only tiers 1 and 3 (`webwrap-file-tier`), so only those
/// deserialize; the rest are the runtime's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u8")]
pub enum Tier {
    /// The `vscodeDev` and `photopea` adapters.
    Verified,
    LaunchQueue,
    NativePicker,
    Upload,
    Unsupported,
}

impl TryFrom<u8> for Tier {
    type Error = &'static str;

    fn try_from(tier: u8) -> Result<Self, Self::Error> {
        match tier {
            1 => Ok(Self::LaunchQueue),
            3 => Ok(Self::Upload),
            _ => Err("a page reports only tier 1 or 3"),
        }
    }
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Verified => "verified write-back",
            Self::LaunchQueue | Self::NativePicker => "best effort",
            Self::Upload => "open only",
            Self::Unsupported => "unsupported",
        }
    }
}

/// How to post a file. `handle_allowed` is false when WebView2 would refuse a
/// handle to its path (`handle_refusal`); `vscodeDev` then reports that error,
/// while a generic site falls back to a buffer, which has no such limits.
pub fn transport(adapter: Adapter, want: Option<Want>, handle_allowed: bool) -> Transport {
    match adapter {
        Adapter::VscodeDev => Transport::Handle,
        Adapter::Photopea => Transport::Buffer,
        Adapter::None if want == Some(Want::Handle) && handle_allowed => Transport::Handle,
        Adapter::None => Transport::Buffer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_generic_site_that_wants_a_handle_and_may_have_one_gets_one() {
        use Transport::{Buffer, Handle};
        for (adapter, want, allowed, expected) in [
            (Adapter::VscodeDev, None, true, Handle),
            (Adapter::VscodeDev, Some(Want::Buffer), false, Handle),
            (Adapter::Photopea, Some(Want::Handle), true, Buffer),
            (Adapter::Photopea, None, true, Buffer),
            (Adapter::None, Some(Want::Handle), true, Handle),
            (Adapter::None, Some(Want::Handle), false, Buffer),
            (Adapter::None, Some(Want::Buffer), true, Buffer),
            (Adapter::None, None, true, Buffer),
        ] {
            assert_eq!(
                transport(adapter, want, allowed),
                expected,
                "{adapter:?} {want:?} {allowed}"
            );
        }
    }

    #[test]
    fn labels() {
        assert_eq!(Tier::Verified.label(), "verified write-back");
        assert_eq!(Tier::LaunchQueue.label(), "best effort");
        assert_eq!(Tier::NativePicker.label(), "best effort");
        assert_eq!(Tier::Upload.label(), "open only");
        assert_eq!(Tier::Unsupported.label(), "unsupported");
    }
}

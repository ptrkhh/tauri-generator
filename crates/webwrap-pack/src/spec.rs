//! Building the app specification from what the user chose.

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use url::Url;
use webwrap_spec::{
    app_identity, canonicalize_url, Adapter, AppSpec, Assoc, DnsMode, SpecError,
    RESERVED_DEVICE_NAMES,
};

use crate::PackError;

/// Every generated app is version 1.0.0 until Android needs monotonic
/// version codes (phase 2, gate 10).
pub const APP_VERSION: &str = "1.0.0";
pub const MAX_ICON_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_ICON_SIDE: u32 = 4096;
/// What the PNG decoder may allocate when decoding: a full-size RGBA icon
/// plus slack. png 0.18.1 inflates an ICC profile only up to this bound and
/// silently drops a larger one (`parse_iccp`), so a profile cannot exhaust
/// memory and is never an error.
const ICON_MAX_ALLOC: u64 = (MAX_ICON_SIDE as u64).pow(2) * 4 + 16 * 1024 * 1024;
/// The same bound for `check_icon`, which needs only the dimensions.
const ICON_CHECK_MAX_ALLOC: u64 = 8 * 1024 * 1024;

/// What the user chose. Everything else in the specification is derived.
#[derive(Debug, Clone)]
pub struct AppInput {
    pub url: String,
    pub display_name: String,
    /// Derived from the website address when absent.
    pub slug: Option<String>,
    pub adapter: Adapter,
    pub file_associations: Vec<Assoc>,
}

/// Build the specification and its serialized bytes.
///
/// The bytes are parsed back with the runtime's own `AppSpec::from_slice`,
/// so the generator can never embed a specification the runtime rejects.
pub fn build_spec(input: &AppInput) -> Result<(AppSpec, Vec<u8>), PackError> {
    let launch_url = canonicalize_url(&input.url).map_err(|error| PackError::Input {
        field: "url",
        reason: error.to_string(),
    })?;
    let origin = Url::parse(&launch_url)
        .expect("a canonical URL parses")
        .origin()
        .ascii_serialization();
    let spec = AppSpec {
        schema_version: 1,
        app_id: app_identity(&launch_url),
        display_name: input.display_name.clone(),
        slug: input
            .slug
            .clone()
            .unwrap_or_else(|| derive_slug(&launch_url)),
        launch_url,
        trusted_origins: vec![origin],
        adapter: input.adapter,
        file_associations: input.file_associations.clone(),
        dns_mode: DnsMode::System,
        app_version: APP_VERSION.into(),
    };
    let bytes = serde_json::to_vec(&spec).expect("AppSpec serializes");
    let checked = AppSpec::from_slice(&bytes).map_err(|error| match error {
        SpecError::Field { field, reason } => PackError::Input { field, reason },
        other => PackError::Input {
            field: "specification",
            reason: other.to_string(),
        },
    })?;
    Ok((checked, bytes))
}

/// The default file name, from the website's host: drop `www.` and the
/// top-level domain, join the rest with hyphens.
///
/// shortcut: no public-suffix list, so `bbc.co.uk` gives `bbc-co`; the user
/// can override the slug. Upgrade if real sites keep producing bad names.
pub fn derive_slug(canonical_url: &str) -> String {
    let host = Url::parse(canonical_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let mut labels: Vec<&str> = host.split('.').collect();
    if labels.len() > 1 {
        labels.pop();
    }
    let mut slug = String::new();
    for c in labels.join("-").chars() {
        let c = if c.is_ascii_alphanumeric() {
            c.to_ascii_lowercase()
        } else {
            '-'
        };
        if !(c == '-' && (slug.is_empty() || slug.ends_with('-'))) {
            slug.push(c);
        }
    }
    slug.truncate(64); // ASCII only, so this cannot split a character
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "app".into()
    } else if RESERVED_DEVICE_NAMES.contains(&slug) {
        format!("{slug}-app")
    } else {
        slug.into()
    }
}

/// Reject icons that are not a square PNG within the size limits, reading
/// only the header so an oversized image is never decoded.
pub fn check_icon(png: &[u8]) -> Result<(), PackError> {
    let bad = |reason: String| PackError::Input {
        field: "icon",
        reason,
    };
    if png.len() > MAX_ICON_BYTES {
        return Err(bad(format!(
            "is {} bytes; the limit is {MAX_ICON_BYTES}",
            png.len()
        )));
    }
    let (width, height) = icon_reader(png, ICON_CHECK_MAX_ALLOC, None)
        .into_dimensions()
        .map_err(|error| bad(format!("is not a readable PNG: {error}")))?;
    if width != height {
        return Err(bad(format!("is {width}x{height}; it must be square")));
    }
    if width > MAX_ICON_SIDE {
        return Err(bad(format!(
            "is {width} pixels wide; the limit is {MAX_ICON_SIDE}"
        )));
    }
    Ok(())
}

/// A PNG reader whose allocations, including the ICC profile it inflates
/// before any pixels, are bounded by `max_alloc`.
fn icon_reader(png: &[u8], max_alloc: u64, max_side: Option<u32>) -> ImageReader<Cursor<&[u8]>> {
    let mut limits = Limits::default();
    limits.max_alloc = Some(max_alloc);
    limits.max_image_width = max_side;
    limits.max_image_height = max_side;
    let mut reader = ImageReader::with_format(Cursor::new(png), ImageFormat::Png);
    reader.limits(limits);
    reader
}

/// Decode an icon under the same bounds `check_icon` enforces.
pub(crate) fn decode_icon(png: &[u8]) -> Result<DynamicImage, PackError> {
    icon_reader(png, ICON_MAX_ALLOC, Some(MAX_ICON_SIDE))
        .decode()
        .map_err(|error| PackError::Input {
            field: "icon",
            reason: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(url: &str) -> AppInput {
        AppInput {
            url: url.into(),
            display_name: "Twitter".into(),
            slug: None,
            adapter: Adapter::None,
            file_associations: vec![],
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::GrayImage::new(width, height)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn slugs_follow_the_spec_examples() {
        for (url, slug) in [
            ("https://twitter.com/", "twitter"),
            ("https://www.photopea.com/", "photopea"),
            ("https://vscode.dev/", "vscode"),
            ("https://mail.google.com/", "mail-google"),
            ("https://bbc.co.uk/", "bbc-co"),
            ("https://example.com:8443/path", "example"),
            ("https://localhost/", "localhost"),
            ("https://xn--bcher-kva.example/", "xn-bcher-kva"),
            ("https://con.com/", "con-app"),
            ("https://192.168.1.1/", "192-168-1"),
        ] {
            assert_eq!(derive_slug(url), slug, "{url}");
        }
    }

    #[test]
    fn long_hosts_truncate_without_a_trailing_hyphen() {
        let host = format!("{}-{}.com", "a".repeat(63), "b".repeat(10));
        let slug = derive_slug(&format!("https://{host}/"));
        assert_eq!(slug, "a".repeat(63));
    }

    #[test]
    fn the_spec_is_built_canonical_and_valid() {
        let (spec, bytes) = build_spec(&input("twitter.com")).unwrap();
        assert_eq!(spec.launch_url, "https://twitter.com/");
        // Pinned by webwrap-spec's identity vectors.
        assert_eq!(spec.app_id, "2fa5e3f40150278f");
        assert_eq!(spec.slug, "twitter");
        assert_eq!(spec.trusted_origins, ["https://twitter.com"]);
        assert_eq!(spec.app_version, APP_VERSION);
        assert_eq!(AppSpec::from_slice(&bytes).unwrap(), spec);
    }

    #[test]
    fn invalid_inputs_name_their_field() {
        let field = |input: AppInput| match build_spec(&input) {
            Err(PackError::Input { field, .. }) => field,
            other => panic!("expected an input error, got {other:?}"),
        };
        assert_eq!(field(input("http://twitter.com/")), "url");
        assert_eq!(field(input("")), "url");
        let mut bad_name = input("twitter.com");
        bad_name.display_name = String::new();
        assert_eq!(field(bad_name), "displayName");
        let mut bad_slug = input("twitter.com");
        bad_slug.slug = Some("Not A Slug".into());
        assert_eq!(field(bad_slug), "slug");
    }

    #[test]
    fn icons_must_be_square_bounded_pngs() {
        assert!(check_icon(&png(64, 64)).is_ok());
        assert!(check_icon(&png(64, 32)).is_err());
        assert!(check_icon(&png(MAX_ICON_SIDE + 1, MAX_ICON_SIDE + 1)).is_err());
        assert!(check_icon(b"GIF89a not a png").is_err());
        assert!(check_icon(&vec![0; MAX_ICON_BYTES + 1]).is_err());
    }
}

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::identity::{app_identity, canonicalize_url};

/// Largest accepted specification resource. A specialized executable carries
/// a handful of short strings; anything approaching this is a defect.
pub const MAX_SPEC_BYTES: usize = 64 * 1024;

/// Names Windows reserves device-wide. The slug becomes a filename, so a slug
/// of "nul" would ask the generator to publish `nul.exe`.
///
/// `com0` and `lpt0` are included because current Microsoft guidance lists
/// them alongside 1-9. `CONIN$` and `CONOUT$` are unreachable here: the slug
/// charset has no `$`.
pub const RESERVED_DEVICE_NAMES: [&str; 24] = [
    "con", "prn", "aux", "nul", "com0", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
    "com8", "com9", "lpt0", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Adapter {
    None,
    Photopea,
    VscodeDev,
}

/// Phase 1 has exactly one DNS mode. Custom DNS over HTTPS is deferred, and
/// the runtime must refuse an unknown value rather than quietly using system
/// DNS, so that phase 2 cannot introduce a silent-fallback regression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DnsMode {
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Assoc {
    pub extension: String,
    pub mime_type: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppSpec {
    pub schema_version: u32,
    pub app_id: String,
    pub display_name: String,
    pub slug: String,
    pub launch_url: String,
    pub trusted_origins: Vec<String>,
    pub adapter: Adapter,
    pub file_associations: Vec<Assoc>,
    pub dns_mode: DnsMode,
    pub app_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    TooLarge {
        bytes: usize,
    },
    NotUtf8,
    Json(String),
    Field {
        field: &'static str,
        reason: String,
    },
    NotCanonical {
        launch_url: String,
        canonical: String,
    },
    IdentityMismatch {
        app_id: String,
        expected: String,
    },
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { bytes } => write!(
                f,
                "the embedded configuration is {bytes} bytes, which exceeds the {MAX_SPEC_BYTES} byte limit"
            ),
            Self::NotUtf8 => f.write_str("the embedded configuration is not valid UTF-8"),
            Self::Json(message) => {
                write!(f, "the embedded configuration is not valid JSON: {message}")
            }
            Self::Field { field, reason } => write!(f, "invalid {field}: {reason}"),
            Self::NotCanonical {
                launch_url,
                canonical,
            } => write!(
                f,
                "launchUrl {launch_url} is not canonical; the canonical form is {canonical}"
            ),
            Self::IdentityMismatch { app_id, expected } => write!(
                f,
                "appId {app_id} does not match the launch URL, which derives {expected}"
            ),
        }
    }
}

impl std::error::Error for SpecError {}

fn field(name: &'static str, reason: impl Into<String>) -> SpecError {
    SpecError::Field {
        field: name,
        reason: reason.into(),
    }
}

/// Code points that render as nothing or reorder what follows them.
///
/// `char::is_control` is false for all of these, so they survive the control
/// check and land in a window title and in the PE `FileDescription`, where a
/// right-to-left override can make a name read as something else entirely.
fn is_deceptive(c: char) -> bool {
    matches!(c,
        '\u{00ad}'                  // soft hyphen
        | '\u{200b}'..='\u{200f}'   // zero width spaces and joiners, LRM, RLM
        | '\u{202a}'..='\u{202e}'   // bidi embedding and override
        | '\u{2060}'..='\u{2064}'   // invisible operators
        | '\u{2066}'..='\u{2069}'   // bidi isolates
        | '\u{feff}'                // byte order mark
    )
}

/// Shared rule for human-readable single-line strings.
///
/// The length bound is in **bytes**, not characters, to match the PE
/// specializer this eventually feeds: `pe.rs` bounds `display-name` with
/// `value.len() > 255`. Bounding characters here instead would accept a
/// 255-character CJK name (765 bytes) that then fails during resource editing,
/// putting the error in the wrong sub-project and far from the field the user
/// typed.
fn check_text(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("must not be empty".into());
    }
    if value.trim().is_empty() {
        return Err("must not be entirely whitespace".into());
    }
    if value.len() > 255 {
        return Err(format!(
            "must be at most 255 bytes, found {} bytes",
            value.len()
        ));
    }
    if value.chars().any(char::is_control) {
        return Err("must not contain control characters".into());
    }
    if value.chars().any(is_deceptive) {
        return Err("must not contain invisible or direction-changing characters".into());
    }
    Ok(())
}

fn check_origin(value: &str) -> Result<(), String> {
    let url = Url::parse(value).map_err(|_| format!("{value} is not a URL"))?;
    if url.scheme() != "https" {
        return Err(format!("{value} is not https"));
    }
    let origin = url.origin();
    if !origin.is_tuple() {
        return Err(format!("{value} has no usable origin"));
    }
    let serialized = origin.ascii_serialization();
    if serialized != value {
        return Err(format!(
            "{value} is not a bare origin; use {serialized} with no path, query, or fragment"
        ));
    }
    Ok(())
}

fn check_assoc(assoc: &Assoc) -> Result<(), String> {
    let extension = &assoc.extension;
    let Some(rest) = extension.strip_prefix('.') else {
        return Err(format!("extension {extension} must start with a dot"));
    };
    if rest.is_empty() || rest.len() > 16 {
        return Err(format!(
            "extension {extension} must have 1 to 16 characters after the dot"
        ));
    }
    if !rest
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return Err(format!(
            "extension {extension} must use only lowercase letters and digits"
        ));
    }
    let mime = &assoc.mime_type;
    let parts: Vec<&str> = mime.split('/').collect();
    if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
        return Err(format!("mimeType {mime} must be type/subtype"));
    }
    // RFC 2045 token characters, further required to begin alphanumerically so
    // that a subtype cannot be something like ".." — this value is written into
    // registry entries by sub-project E and into an Android manifest later.
    for part in &parts {
        if !part.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            return Err(format!(
                "mimeType {mime} must have parts that start with a letter or digit"
            ));
        }
        if !part.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(
                    c,
                    '!' | '#'
                        | '$'
                        | '%'
                        | '&'
                        | '\''
                        | '*'
                        | '+'
                        | '-'
                        | '.'
                        | '^'
                        | '_'
                        | '`'
                        | '|'
                        | '~'
                )
        }) {
            return Err(format!(
                "mimeType {mime} contains a character that is not allowed"
            ));
        }
    }
    check_text(&assoc.description).map_err(|reason| format!("description {reason}"))?;
    Ok(())
}

impl AppSpec {
    /// Check every field in isolation. Cross-field invariants live in
    /// `validate`.
    ///
    /// Deliberately private. It does not check that `launch_url` is canonical,
    /// which is the property everything else hangs off, so a caller reaching
    /// for "just the field checks" would get a silent pass on the one field
    /// that matters most. `validate` is the only entry point.
    fn validate_fields(&self) -> Result<(), SpecError> {
        if self.schema_version != 1 {
            return Err(field(
                "schemaVersion",
                format!("must be 1, found {}", self.schema_version),
            ));
        }

        if self.app_id.len() != 16
            || !self
                .app_id
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        {
            return Err(field(
                "appId",
                "must be exactly 16 lowercase hexadecimal characters",
            ));
        }

        check_text(&self.display_name).map_err(|reason| field("displayName", reason))?;

        // Bounded independently of MAX_SPEC_BYTES so the field stays symmetric
        // with displayName and slug. Well beyond any real launch URL.
        if self.launch_url.len() > 2048 {
            return Err(field("launchUrl", "must be at most 2048 bytes"));
        }

        let slug = &self.slug;
        if slug.is_empty() || slug.len() > 64 {
            return Err(field("slug", "must be 1 to 64 characters"));
        }
        if !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(field(
                "slug",
                "must use only lowercase letters, digits, and hyphens",
            ));
        }
        if slug.starts_with('-') || slug.ends_with('-') {
            return Err(field("slug", "must not start or end with a hyphen"));
        }
        if RESERVED_DEVICE_NAMES.contains(&slug.as_str()) {
            return Err(field(
                "slug",
                format!("{slug} is a name Windows reserves for a device"),
            ));
        }

        if self.trusted_origins.is_empty() {
            return Err(field("trustedOrigins", "must list at least one origin"));
        }
        let mut seen = BTreeSet::new();
        for origin in &self.trusted_origins {
            check_origin(origin).map_err(|reason| field("trustedOrigins", reason))?;
            if !seen.insert(origin.as_str()) {
                return Err(field(
                    "trustedOrigins",
                    format!("{origin} is listed more than once"),
                ));
            }
        }

        // Every component must be plain digits before parsing: Rust's unsigned
        // FromStr accepts a leading `+`, so "+1.0.0" would otherwise pass and
        // be written verbatim into the PE FileVersion string.
        let components: Vec<&str> = self.app_version.split('.').collect();
        if components.len() != 3
            || components.iter().any(|part| {
                part.is_empty()
                    || !part.chars().all(|c| c.is_ascii_digit())
                    || part.parse::<u16>().is_err()
            })
        {
            return Err(field(
                "appVersion",
                "must be three numbers separated by dots, each at most 65535",
            ));
        }

        let mut extensions = BTreeSet::new();
        for assoc in &self.file_associations {
            check_assoc(assoc).map_err(|reason| field("fileAssociations", reason))?;
            if !extensions.insert(assoc.extension.as_str()) {
                return Err(field(
                    "fileAssociations",
                    format!("{} is claimed more than once", assoc.extension),
                ));
            }
        }

        Ok(())
    }

    /// Check every field, then the two cross-field invariants.
    ///
    /// Field errors are reported first so a message names the smallest cause.
    pub fn validate(&self) -> Result<(), SpecError> {
        self.validate_fields()?;

        // Invariant 1: the launch URL is already canonical. If it is not, the
        // specification was produced by a path that skipped canonicalization.
        let canonical = canonicalize_url(&self.launch_url)
            .map_err(|error| field("launchUrl", error.to_string()))?;
        if canonical != self.launch_url {
            return Err(SpecError::NotCanonical {
                launch_url: self.launch_url.clone(),
                canonical,
            });
        }

        // Invariant 2: the identity matches the URL it was derived from. This
        // keeps the WebView2 data directory from drifting away from the site
        // it belongs to, and catches a hand-edited specification.
        let expected = app_identity(&canonical);
        if expected != self.app_id {
            return Err(SpecError::IdentityMismatch {
                app_id: self.app_id.clone(),
                expected,
            });
        }

        Ok(())
    }

    /// Parse and fully validate a specification resource.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, SpecError> {
        if bytes.len() > MAX_SPEC_BYTES {
            return Err(SpecError::TooLarge { bytes: bytes.len() });
        }
        let text = std::str::from_utf8(bytes).map_err(|_| SpecError::NotUtf8)?;
        let spec: Self =
            serde_json::from_str(text).map_err(|error| SpecError::Json(error.to_string()))?;
        spec.validate()?;
        Ok(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A specification that passes every field check. Field tests mutate one
    /// field of this so a failure names exactly one cause.
    fn valid() -> AppSpec {
        AppSpec {
            schema_version: 1,
            app_id: "873756fbe97a1384".into(),
            display_name: "Photopea".into(),
            slug: "photopea".into(),
            launch_url: "https://www.photopea.com/".into(),
            trusted_origins: vec!["https://www.photopea.com".into()],
            adapter: Adapter::Photopea,
            file_associations: vec![Assoc {
                extension: ".psd".into(),
                mime_type: "image/vnd.adobe.photoshop".into(),
                description: "Photoshop document".into(),
            }],
            dns_mode: DnsMode::System,
            app_version: "1.0.0".into(),
        }
    }

    fn field_of(result: Result<(), SpecError>) -> &'static str {
        match result {
            Err(SpecError::Field { field, .. }) => field,
            other => panic!("expected a field error, got {other:?}"),
        }
    }

    #[test]
    fn the_valid_fixture_passes_every_field_check() {
        assert_eq!(valid().validate_fields(), Ok(()));
    }

    #[test]
    fn schema_version_must_be_exactly_one() {
        for version in [0, 2, 99] {
            let mut spec = valid();
            spec.schema_version = version;
            assert_eq!(field_of(spec.validate_fields()), "schemaVersion");
        }
    }

    #[test]
    fn app_id_must_be_sixteen_lowercase_hex_characters() {
        for bad in [
            "",
            "873756fbe97a138",
            "873756fbe97a13845",
            "873756FBE97A1384",
            "873756fbe97a138g",
        ] {
            let mut spec = valid();
            spec.app_id = bad.into();
            assert_eq!(field_of(spec.validate_fields()), "appId", "{bad}");
        }
    }

    #[test]
    fn display_name_rejects_empty_control_characters_and_overlong() {
        for bad in ["", "   ", "Photo\npea", "Photo\0pea", &"a".repeat(256)] {
            let mut spec = valid();
            spec.display_name = bad.into();
            assert_eq!(field_of(spec.validate_fields()), "displayName");
        }
        let mut spec = valid();
        spec.display_name = "a".repeat(255);
        assert_eq!(spec.validate_fields(), Ok(()));
    }

    #[test]
    fn display_name_is_bounded_in_bytes_not_characters() {
        // The PE specializer bounds display-name with value.len() > 255, so a
        // character bound here would accept names it then rejects. 255 CJK
        // characters are 765 bytes. An ASCII-only fixture cannot catch this,
        // which is why it is tested explicitly.
        let cjk = "\u{6f22}".repeat(255);
        assert_eq!(cjk.chars().count(), 255);
        assert_eq!(cjk.len(), 765);
        let mut spec = valid();
        spec.display_name = cjk;
        assert_eq!(field_of(spec.validate_fields()), "displayName");

        // 85 CJK characters are exactly 255 bytes and must be accepted.
        let mut spec = valid();
        spec.display_name = "\u{6f22}".repeat(85);
        assert_eq!(spec.display_name.len(), 255);
        assert_eq!(spec.validate_fields(), Ok(()));
    }

    #[test]
    fn display_name_rejects_invisible_and_direction_changing_characters() {
        // None of these are control characters, so they survive that check and
        // would reach the window title and the PE FileDescription, where a
        // right-to-left override makes a name read as something else.
        for bad in [
            "Photo\u{202e}pea",
            "Photo\u{200b}pea",
            "Photo\u{00ad}pea",
            "\u{feff}Photopea",
            "Photo\u{2066}pea",
        ] {
            let mut spec = valid();
            spec.display_name = bad.into();
            assert_eq!(
                field_of(spec.validate_fields()),
                "displayName",
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn launch_url_is_length_bounded() {
        let mut spec = valid();
        spec.launch_url = format!("https://example.test/{}", "a".repeat(2048));
        assert_eq!(field_of(spec.validate_fields()), "launchUrl");
    }

    #[test]
    fn slug_is_filename_safe_and_rejects_windows_device_names() {
        for bad in [
            "",
            "Photopea",
            "photo pea",
            "photo_pea",
            "photo.pea",
            "-photopea",
            "photopea-",
            "con",
            "nul",
            "com1",
            "lpt9",
            "aux",
            &"a".repeat(65),
        ] {
            let mut spec = valid();
            spec.slug = bad.into();
            assert_eq!(field_of(spec.validate_fields()), "slug", "{bad}");
        }
        for good in ["photopea", "vscode-dev", "a", "app2"] {
            let mut spec = valid();
            spec.slug = good.into();
            assert_eq!(spec.validate_fields(), Ok(()), "{good}");
        }
    }

    #[test]
    fn trusted_origins_must_be_non_empty_bare_https_origins_without_duplicates() {
        for bad in [
            vec![],
            vec!["https://www.photopea.com/".to_string()],
            vec!["http://www.photopea.com".to_string()],
            vec!["https://www.photopea.com/path".to_string()],
            vec!["https://www.photopea.com?q=1".to_string()],
            vec!["www.photopea.com".to_string()],
            vec![
                "https://www.photopea.com".to_string(),
                "https://www.photopea.com".to_string(),
            ],
        ] {
            let mut spec = valid();
            spec.trusted_origins = bad.clone();
            assert_eq!(
                field_of(spec.validate_fields()),
                "trustedOrigins",
                "{bad:?}"
            );
        }
        let mut spec = valid();
        spec.trusted_origins = vec![
            "https://www.photopea.com".into(),
            "https://example.test:8443".into(),
        ];
        assert_eq!(spec.validate_fields(), Ok(()));
    }

    #[test]
    fn app_version_is_three_u16_components() {
        for bad in [
            "",
            "1",
            "1.0",
            "1.0.0.0",
            "1.0.x",
            "65536.0.0",
            "1..0",
            "v1.0.0",
        ] {
            let mut spec = valid();
            spec.app_version = bad.into();
            assert_eq!(field_of(spec.validate_fields()), "appVersion", "{bad}");
        }
        for good in ["0.0.0", "1.0.0", "65535.65535.65535"] {
            let mut spec = valid();
            spec.app_version = good.into();
            assert_eq!(spec.validate_fields(), Ok(()), "{good}");
        }
    }

    #[test]
    fn app_version_rejects_a_signed_component() {
        // Rust's unsigned FromStr accepts a leading `+`, so "+1".parse::<u16>()
        // is Ok(1). Without an explicit digit check the value would reach the
        // PE FileVersion string as "+1.0.0".
        assert_eq!("+1".parse::<u16>(), Ok(1));
        for bad in ["+1.0.0", "1.+0.0", "1.0.+0", "-1.0.0", "1.-0.0"] {
            let mut spec = valid();
            spec.app_version = bad.into();
            assert_eq!(field_of(spec.validate_fields()), "appVersion", "{bad}");
        }
    }

    #[test]
    fn boundary_lengths_are_accepted_not_just_rejected() {
        let mut spec = valid();
        spec.slug = "a".repeat(64);
        assert_eq!(spec.validate_fields(), Ok(()), "64-character slug");

        let mut spec = valid();
        spec.file_associations[0].extension = format!(".{}", "a".repeat(16));
        assert_eq!(spec.validate_fields(), Ok(()), "16-character extension");

        let mut spec = valid();
        spec.file_associations[0].description = "a".repeat(255);
        assert_eq!(spec.validate_fields(), Ok(()), "255-byte description");
    }

    #[test]
    fn mime_types_must_be_token_shaped() {
        for bad in [
            "image/..",
            "image/-x",
            "../x",
            "image/x y",
            "image/x\u{00e9}",
            "image/x;charset=y",
        ] {
            let mut spec = valid();
            spec.file_associations[0].mime_type = bad.into();
            assert_eq!(
                field_of(spec.validate_fields()),
                "fileAssociations",
                "{bad}"
            );
        }
        for good in [
            "image/vnd.adobe.photoshop",
            "text/markdown",
            "text/x-c++src",
        ] {
            let mut spec = valid();
            spec.file_associations[0].mime_type = good.into();
            assert_eq!(spec.validate_fields(), Ok(()), "{good}");
        }
    }

    #[test]
    fn assoc_also_rejects_unknown_fields() {
        let json = r#"{"extension":".psd","mimeType":"image/x","description":"d","extra":1}"#;
        assert!(serde_json::from_str::<Assoc>(json).is_err());
    }

    #[test]
    fn file_associations_may_be_empty_but_must_be_well_formed_and_unique() {
        let mut spec = valid();
        spec.file_associations = vec![];
        assert_eq!(spec.validate_fields(), Ok(()));

        for bad_extension in [
            "",
            "psd",
            ".",
            ".PSD",
            ".ps d",
            &format!(".{}", "a".repeat(17)),
        ] {
            let mut spec = valid();
            spec.file_associations[0].extension = bad_extension.into();
            assert_eq!(field_of(spec.validate_fields()), "fileAssociations");
        }
        for bad_mime in [
            "",
            "image",
            "image/",
            "/photoshop",
            "image / x",
            "image/x/y",
        ] {
            let mut spec = valid();
            spec.file_associations[0].mime_type = bad_mime.into();
            assert_eq!(field_of(spec.validate_fields()), "fileAssociations");
        }
        let mut spec = valid();
        spec.file_associations[0].description = String::new();
        assert_eq!(field_of(spec.validate_fields()), "fileAssociations");

        let mut spec = valid();
        let duplicate = spec.file_associations[0].clone();
        spec.file_associations.push(duplicate);
        assert_eq!(field_of(spec.validate_fields()), "fileAssociations");
    }

    #[test]
    fn json_round_trip_uses_camel_case_and_rejects_unknown_fields() {
        let json = serde_json::to_string(&valid()).unwrap();
        assert!(json.contains("\"schemaVersion\":1"), "{json}");
        assert!(json.contains("\"appId\""), "{json}");
        assert!(json.contains("\"mimeType\""), "{json}");
        assert!(json.contains("\"adapter\":\"photopea\""), "{json}");
        assert!(json.contains("\"dnsMode\":\"system\""), "{json}");
        assert_eq!(serde_json::from_str::<AppSpec>(&json).unwrap(), valid());

        let with_extra = json.replace("{\"schemaVersion\"", "{\"surprise\":1,\"schemaVersion\"");
        assert!(serde_json::from_str::<AppSpec>(&with_extra).is_err());
    }

    #[test]
    fn dns_mode_accepts_only_system_and_adapter_names_are_camel_case() {
        assert!(serde_json::from_str::<DnsMode>("\"system\"").is_ok());
        assert!(serde_json::from_str::<DnsMode>("\"doh\"").is_err());
        assert!(serde_json::from_str::<DnsMode>("\"custom\"").is_err());
        assert_eq!(
            serde_json::from_str::<Adapter>("\"vscodeDev\"").unwrap(),
            Adapter::VscodeDev
        );
        assert_eq!(
            serde_json::from_str::<Adapter>("\"none\"").unwrap(),
            Adapter::None
        );
        assert!(serde_json::from_str::<Adapter>("\"vscode_dev\"").is_err());
    }

    #[test]
    fn validate_accepts_a_self_consistent_specification() {
        assert_eq!(valid().validate(), Ok(()));
    }

    #[test]
    fn launch_url_must_already_be_canonical() {
        let mut spec = valid();
        spec.launch_url = "https://www.photopea.com".into(); // missing trailing slash
        match spec.validate() {
            Err(SpecError::NotCanonical { canonical, .. }) => {
                assert_eq!(canonical, "https://www.photopea.com/");
            }
            other => panic!("expected NotCanonical, got {other:?}"),
        }

        let mut spec = valid();
        spec.launch_url = "http://www.photopea.com/".into();
        assert!(matches!(
            spec.validate(),
            Err(SpecError::Field {
                field: "launchUrl",
                ..
            })
        ));
    }

    #[test]
    fn app_id_must_match_the_launch_url() {
        let mut spec = valid();
        spec.launch_url = "https://twitter.com/".into();
        // app_id still belongs to photopea, so identity no longer matches.
        match spec.validate() {
            Err(SpecError::IdentityMismatch { expected, .. }) => {
                assert_eq!(expected, "2fa5e3f40150278f");
            }
            other => panic!("expected IdentityMismatch, got {other:?}"),
        }
    }

    #[test]
    fn field_errors_are_reported_before_invariants() {
        let mut spec = valid();
        spec.schema_version = 2;
        spec.app_id = "0000000000000000".into();
        assert_eq!(field_of(spec.validate()), "schemaVersion");
    }

    #[test]
    fn from_slice_parses_validates_and_reports_each_failure_kind() {
        let json = serde_json::to_vec(&valid()).unwrap();
        assert_eq!(AppSpec::from_slice(&json).unwrap(), valid());

        assert_eq!(
            AppSpec::from_slice(&vec![b'a'; MAX_SPEC_BYTES + 1]),
            Err(SpecError::TooLarge {
                bytes: MAX_SPEC_BYTES + 1
            })
        );
        assert_eq!(AppSpec::from_slice(&[0xff, 0xfe]), Err(SpecError::NotUtf8));
        assert!(matches!(
            AppSpec::from_slice(b"{not json"),
            Err(SpecError::Json(_))
        ));
        assert!(matches!(
            AppSpec::from_slice(b"[1,2,3]"),
            Err(SpecError::Json(_))
        ));
        assert!(matches!(
            AppSpec::from_slice(b"{}"),
            Err(SpecError::Json(_))
        ));

        let mut bad = valid();
        bad.slug = "con".into();
        let bad = serde_json::to_vec(&bad).unwrap();
        assert_eq!(field_of(AppSpec::from_slice(&bad).map(|_| ())), "slug");
    }

    #[test]
    fn an_empty_resource_is_rejected_rather_than_treated_as_a_default() {
        assert!(AppSpec::from_slice(b"").is_err());
    }
}

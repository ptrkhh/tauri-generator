# WebWrap App Spec and Minimal Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `webwrap-spec` crate (app specification, validation, URL canonicalization, stable identity, navigation policy) and the minimal `webwrap-runtime` Windows wrapper that reads its own embedded specification, isolates its WebView2 storage per app identity, and enforces navigation policy.

**Architecture:** Two crates. `webwrap-spec` is pure, cross-platform Rust with `#![forbid(unsafe_code)]`, so every branch in the system is unit-tested under WSL. `webwrap-runtime` is a Windows-only Tauri binary containing nothing but Win32 and Tauri glue; it reads an `RT_RCDATA` resource named `WEBWRAP_APP_SPEC` from its own loaded image. A `--inspect-self` JSON mode makes the runtime's own resolved configuration assertable by automated tests rather than eyeballed.

**Tech Stack:** Rust 2021, `serde` 1.0.229, `serde_json` 1.0.151, `sha2` 0.10.9, `url` 2.5.8, `tauri` 2.11.5, `tauri-build` 2.6.3, `windows` 0.61.3, WebView2.

**Spec:** [`docs/superpowers/specs/2026-09-16-webwrap-spec-and-minimal-runtime-design.md`](../specs/2026-09-16-webwrap-spec-and-minimal-runtime-design.md)

## Global Constraints

- **Tasks 1-4 run under WSL.** `cargo test -p webwrap-spec` with no `--target` flag. No Windows needed.
- **Tasks 5-9 run on Windows.** Sub-project B is `compile_error!` off Windows.
- **Always pass `--target` explicitly on Windows.** Both `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`. A naive `cargo build` on the ARM64 host silently produces an aarch64 binary. `--target` is also what routes artifacts into `target/<triple>/` instead of colliding in `target/debug`. Do **not** add `targets = [...]` to `rust-toolchain.toml`: that key only controls which targets rustup installs, it does not change the default build target, and it would make the WSL toolchain download two Windows standard libraries it never uses.
- **No new dependency trees.** Every crate listed above is already in `Cargo.lock`. Pin with `=` exactly as the existing spikes do.
- **`spikes/` is untouched by this plan.** Task 6 onward depends on the `webwrap-pe-specialize` binary still existing.
- **Resource contract, fixed by the existing spike:** type `RT_RCDATA` (ID 10), name `WEBWRAP_APP_SPEC` (a string name, not an ID), language ID `0x0409` (en-US). The runtime must look it up with exactly these values.
- **Specification size cap:** 64 KiB.
- **`schemaVersion` must equal `1` exactly.** No range check, no forward compatibility.
- **`dnsMode` accepts only `system`** in phase 1.
- **Exit codes:** `0` success, `2` usage error, `1` runtime failure.
- **Identity is `sha256(canonical_url)` truncated to 16 lowercase hex characters.** Never a non-cryptographic hasher; `DefaultHasher` output is not stable across Rust releases and identity must be stable forever.
- **Gates closed by this plan:** 11 and 14 only. Gates 1, 4, 5, 6, 7, 8 stay open and belong to sub-projects C, D, E. Do not describe them as proven.

---

## File Structure

| File | Responsibility |
| --- | --- |
| `Cargo.toml` | modify: workspace members glob |
| `rust-toolchain.toml` | modify: add both Windows target triples |
| `crates/webwrap-spec/Cargo.toml` | create: pure crate manifest |
| `crates/webwrap-spec/src/lib.rs` | create: `forbid(unsafe_code)`, module wiring, re-exports |
| `crates/webwrap-spec/src/identity.rs` | create: `canonicalize_url`, `app_identity`, `IdentityError` |
| `crates/webwrap-spec/src/policy.rs` | create: `NavEvent`, `Verdict`, `nav_verdict`, `is_trusted_origin` |
| `crates/webwrap-spec/src/schema.rs` | create: `AppSpec`, `Assoc`, `Adapter`, `DnsMode`, `SpecError`, validation |
| `crates/webwrap-spec/tests/vectors.rs` | create: permanent canonicalization and identity contract vectors |
| `crates/webwrap-runtime/Cargo.toml` | create: Windows Tauri binary manifest |
| `crates/webwrap-runtime/build.rs` | create: placeholder ICO + `tauri_build` |
| `crates/webwrap-runtime/tauri.conf.json` | create: product name, version, identifier |
| `crates/webwrap-runtime/ui/index.html` | create: minimal local asset root |
| `crates/webwrap-runtime/src/main.rs` | create: argument parsing, data dir, dispatch, window |
| `crates/webwrap-runtime/src/embedded_spec.rs` | create: Win32 resource read (contains `unsafe`) |
| `crates/webwrap-runtime/src/fatal.rs` | create: `MessageBoxW` error surface (contains `unsafe`) |
| `crates/webwrap-runtime/src/inspect.rs` | create: `--inspect-self` JSON output |
| `crates/webwrap-runtime/tests/inspect_self.rs` | create: native integration test |
| `docs/evidence/2026-09-16-webwrap-spec-and-minimal-runtime.md` | create: acceptance evidence |

`policy.rs` deliberately does **not** import `schema.rs`. `is_trusted_origin` takes `&[String]`, not `&AppSpec`, so the two modules stay independent and `policy` can be implemented and reviewed before `schema` exists.

---

### Task 1: Workspace wiring, URL canonicalization, and app identity

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/webwrap-spec/Cargo.toml`
- Create: `crates/webwrap-spec/src/lib.rs`
- Create: `crates/webwrap-spec/src/identity.rs`
- Test: `crates/webwrap-spec/tests/vectors.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `webwrap_spec::canonicalize_url(input: &str) -> Result<String, IdentityError>`; `webwrap_spec::app_identity(canonical_url: &str) -> String`; `webwrap_spec::IdentityError` (variants `Unparseable`, `NotHttps`, `NoHost`, `HasCredentials`, all `Display`).

- [ ] **Step 1: Create the crate manifest and workspace wiring**

`Cargo.toml` — replace the `members` list:

```toml
[workspace]
members = ["crates/*", "spikes/*"]
resolver = "2"
```

Leave `rust-toolchain.toml` unchanged. Target installation belongs to Task 5, on Windows.

`crates/webwrap-spec/Cargo.toml`:

```toml
[package]
name = "webwrap-spec"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "=1.0.229", features = ["derive"] }
serde_json = "=1.0.151"
sha2 = "=0.10.9"
url = "=2.5.8"
```

`crates/webwrap-spec/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod identity;

pub use identity::{app_identity, canonicalize_url, IdentityError};
```

- [ ] **Step 2: Write the failing contract vectors**

`crates/webwrap-spec/tests/vectors.rs`:

```rust
use webwrap_spec::{app_identity, canonicalize_url, IdentityError};

/// These vectors are a permanent contract. An app's identity decides whether
/// regenerating a URL updates the existing app or creates a second one, so
/// changing an expected value here breaks every app already generated. If a
/// vector fails, the implementation is wrong, not the vector.
#[test]
fn canonical_urls_and_identities_are_stable() {
    let cases = [
        ("twitter.com", "https://twitter.com/", "2fa5e3f40150278f"),
        (
            "https://www.photopea.com",
            "https://www.photopea.com/",
            "873756fbe97a1384",
        ),
        (
            "HTTPS://Twitter.COM:443/Home?a=1#x",
            "https://twitter.com/Home?a=1#x",
            "dee9ebc42c95a754",
        ),
        ("vscode.dev", "https://vscode.dev/", "3c0f4a022526c2c8"),
        (
            "www.facebook.com",
            "https://www.facebook.com/",
            "0b2df0f635584f42",
        ),
    ];
    for (input, expected_canonical, expected_identity) in cases {
        let canonical = canonicalize_url(input).expect(input);
        assert_eq!(canonical, expected_canonical, "canonicalizing {input}");
        assert_eq!(
            app_identity(&canonical),
            expected_identity,
            "identity of {input}"
        );
    }
}

#[test]
fn different_urls_get_different_identities() {
    let twitter = app_identity(&canonicalize_url("twitter.com").unwrap());
    let facebook = app_identity(&canonicalize_url("www.facebook.com").unwrap());
    assert_ne!(twitter, facebook);
}

#[test]
fn identity_is_sixteen_lowercase_hex_characters() {
    let identity = app_identity(&canonicalize_url("twitter.com").unwrap());
    assert_eq!(identity.len(), 16);
    assert!(identity
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

#[test]
fn canonicalization_is_idempotent() {
    let once = canonicalize_url("twitter.com").unwrap();
    let twice = canonicalize_url(&once).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn non_https_and_malformed_inputs_are_rejected() {
    assert_eq!(
        canonicalize_url("http://twitter.com"),
        Err(IdentityError::NotHttps)
    );
    assert_eq!(
        canonicalize_url("ftp://twitter.com"),
        Err(IdentityError::NotHttps)
    );
    assert_eq!(
        canonicalize_url("https://user:pw@twitter.com/"),
        Err(IdentityError::HasCredentials)
    );
    assert!(canonicalize_url("").is_err());
    assert!(canonicalize_url("   ").is_err());
    assert!(canonicalize_url("https://").is_err());
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p webwrap-spec`
Expected: FAIL — `unresolved import` / `cannot find function canonicalize_url`, because `identity.rs` does not exist yet.

- [ ] **Step 4: Implement canonicalization and identity**

`crates/webwrap-spec/src/identity.rs`:

```rust
use sha2::{Digest, Sha256};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    Unparseable,
    NotHttps,
    NoHost,
    HasCredentials,
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unparseable => f.write_str("the website address could not be understood"),
            Self::NotHttps => f.write_str("only https website addresses are supported"),
            Self::NoHost => f.write_str("the website address has no host name"),
            Self::HasCredentials => {
                f.write_str("the website address must not contain a user name or password")
            }
        }
    }
}

impl std::error::Error for IdentityError {}

/// Canonicalize a website address into the exact string that identity is
/// derived from.
///
/// A missing scheme becomes `https`. Any scheme that is already present and is
/// not `https` is rejected rather than upgraded: silently rewriting
/// `http://` would change which server the user asked for.
pub fn canonicalize_url(input: &str) -> Result<String, IdentityError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(IdentityError::Unparseable);
    }
    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let url = Url::parse(&candidate).map_err(|_| IdentityError::Unparseable)?;
    if url.scheme() != "https" {
        return Err(IdentityError::NotHttps);
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(IdentityError::NoHost);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(IdentityError::HasCredentials);
    }
    // The url crate already lowercases the host, applies IDN punycode, strips
    // the default 443 port, and normalizes an empty path to "/", while
    // preserving path, query, and fragment case.
    Ok(url.to_string())
}

/// Derive the stable app identity from an already-canonical URL.
///
/// Truncating SHA-256 to 8 bytes gives 64 bits, which puts a birthday
/// collision far beyond any plausible number of generated apps. A
/// cryptographic digest is required: `DefaultHasher` output is explicitly not
/// stable across Rust releases, and this value must never change for a given
/// URL.
pub fn app_identity(canonical_url: &str) -> String {
    let digest = Sha256::digest(canonical_url.as_bytes());
    digest[..8].iter().map(|byte| format!("{byte:02x}")).collect()
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p webwrap-spec`
Expected: PASS, 5 tests.

If `is_none_or` is unavailable, replace with `!url.host_str().is_some_and(|host| !host.is_empty())`.

- [ ] **Step 6: Run the quality gates**

Run: `cargo clippy -p webwrap-spec --all-targets -- -D warnings`
Run: `cargo fmt --all -- --check`
Expected: both pass.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/webwrap-spec
git commit -m "feat(spec): canonicalize URLs and derive stable app identity"
```

---

### Task 2: Navigation policy

**Files:**
- Create: `crates/webwrap-spec/src/policy.rs`
- Modify: `crates/webwrap-spec/src/lib.rs`

**Interfaces:**
- Consumes: nothing. This module deliberately does not depend on `schema.rs`.
- Produces: `NavEvent` (`TopLevel`, `NewWindow`); `Verdict` (`Allow`, `Block`, `OpenExternally`) with `Verdict::is_allow(self) -> bool`; `nav_verdict(url: &str, event: NavEvent) -> Verdict`; `is_trusted_origin(trusted_origins: &[String], url: &str) -> bool`.

- [ ] **Step 1: Write the failing test**

Create `crates/webwrap-spec/src/policy.rs` containing **only** this test module. Step 3 prepends the implementation above it.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_top_level_navigation_is_allowed_including_untrusted_origins() {
        // Login redirects leave the trusted origin and must still work.
        for url in [
            "https://www.photopea.com/",
            "https://accounts.google.com/o/oauth2/auth",
            "https://login.microsoftonline.com/common/oauth2/authorize",
        ] {
            assert_eq!(nav_verdict(url, NavEvent::TopLevel), Verdict::Allow, "{url}");
        }
    }

    #[test]
    fn non_https_top_level_navigation_is_blocked() {
        for url in [
            "http://www.photopea.com/",
            "file:///C:/Windows/System32/drivers/etc/hosts",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "ftp://example.com/",
            "not a url at all",
        ] {
            assert_eq!(nav_verdict(url, NavEvent::TopLevel), Verdict::Block, "{url}");
        }
    }

    #[test]
    fn every_new_window_opens_externally_regardless_of_scheme() {
        for url in ["https://example.com/", "http://example.com/", "garbage"] {
            assert_eq!(
                nav_verdict(url, NavEvent::NewWindow),
                Verdict::OpenExternally,
                "{url}"
            );
        }
    }

    #[test]
    fn only_allow_is_allow() {
        assert!(Verdict::Allow.is_allow());
        assert!(!Verdict::Block.is_allow());
        assert!(!Verdict::OpenExternally.is_allow());
    }

    #[test]
    fn trusted_origin_matches_on_origin_not_on_url_prefix() {
        let trusted = vec!["https://www.photopea.com".to_string()];
        assert!(is_trusted_origin(&trusted, "https://www.photopea.com/"));
        assert!(is_trusted_origin(
            &trusted,
            "https://www.photopea.com/deep/path?q=1#frag"
        ));
        // A host that merely starts with the trusted host must not match.
        assert!(!is_trusted_origin(
            &trusted,
            "https://www.photopea.com.evil.test/"
        ));
        assert!(!is_trusted_origin(&trusted, "https://photopea.com/"));
        assert!(!is_trusted_origin(&trusted, "http://www.photopea.com/"));
        assert!(!is_trusted_origin(&trusted, "https://www.photopea.com:8443/"));
        assert!(!is_trusted_origin(&trusted, "garbage"));
        assert!(!is_trusted_origin(&[], "https://www.photopea.com/"));
    }

    #[test]
    fn non_default_port_origins_match_only_themselves() {
        let trusted = vec!["https://example.test:8443".to_string()];
        assert!(is_trusted_origin(&trusted, "https://example.test:8443/x"));
        assert!(!is_trusted_origin(&trusted, "https://example.test/x"));
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p webwrap-spec policy`
Expected: FAIL — `file not found for module policy` or `cannot find type NavEvent`.

- [ ] **Step 3: Implement the policy**

Prepend to `crates/webwrap-spec/src/policy.rs`, above the test module:

```rust
use url::Url;

/// Which kind of navigation is being decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavEvent {
    TopLevel,
    NewWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Block,
    OpenExternally,
}

impl Verdict {
    pub fn is_allow(self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// Decide a navigation.
///
/// This takes no `AppSpec` on purpose. `trustedOrigins` gates the native file
/// bridge, not navigation. Gating navigation on trusted origins would block
/// every OAuth redirect to an identity provider and break login, so the
/// navigation verdict depends only on the scheme and the event kind.
pub fn nav_verdict(url: &str, event: NavEvent) -> Verdict {
    if matches!(event, NavEvent::NewWindow) {
        return Verdict::OpenExternally;
    }
    match Url::parse(url) {
        Ok(parsed) if parsed.scheme() == "https" => Verdict::Allow,
        _ => Verdict::Block,
    }
}

/// Whether a URL's origin is one the native file bridge may be handed to.
///
/// Compares serialized origins, never string prefixes: a prefix comparison
/// would accept `https://www.photopea.com.evil.test`.
pub fn is_trusted_origin(trusted_origins: &[String], url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    let origin = parsed.origin();
    if !origin.is_tuple() {
        return false;
    }
    trusted_origins.contains(&origin.ascii_serialization())
}
```

`crates/webwrap-spec/src/lib.rs` — add the module and re-exports:

```rust
#![forbid(unsafe_code)]

pub mod identity;
pub mod policy;

pub use identity::{app_identity, canonicalize_url, IdentityError};
pub use policy::{is_trusted_origin, nav_verdict, NavEvent, Verdict};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p webwrap-spec`
Expected: PASS, 11 tests (6 policy unit tests plus the 5 contract vectors).

- [ ] **Step 5: Run the quality gates**

Run: `cargo clippy -p webwrap-spec --all-targets -- -D warnings`
Run: `cargo fmt --all -- --check`

- [ ] **Step 6: Commit**

```bash
git add crates/webwrap-spec/src/policy.rs crates/webwrap-spec/src/lib.rs
git commit -m "feat(spec): decide navigation by scheme and event kind"
```

---

### Task 3: App specification types and field validation

**Files:**
- Create: `crates/webwrap-spec/src/schema.rs`
- Modify: `crates/webwrap-spec/src/lib.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks in this task's own code.
- Produces: `AppSpec` with public fields `schema_version: u32`, `app_id: String`, `display_name: String`, `slug: String`, `launch_url: String`, `trusted_origins: Vec<String>`, `adapter: Adapter`, `file_associations: Vec<Assoc>`, `dns_mode: DnsMode`, `app_version: String`; `Assoc { extension, mime_type, description }` all `String`; `Adapter` (`None`, `Photopea`, `VscodeDev`); `DnsMode` (`System`); `SpecError`; `AppSpec::validate_fields(&self) -> Result<(), SpecError>`.

- [ ] **Step 1: Write the failing test**

Create `crates/webwrap-spec/src/schema.rs` containing only this test module for now:

```rust
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
        for bad in ["", "Photo\npea", "Photo\0pea", &"a".repeat(256)] {
            let mut spec = valid();
            spec.display_name = bad.into();
            assert_eq!(field_of(spec.validate_fields()), "displayName");
        }
        let mut spec = valid();
        spec.display_name = "a".repeat(255);
        assert_eq!(spec.validate_fields(), Ok(()));
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
            assert_eq!(field_of(spec.validate_fields()), "trustedOrigins", "{bad:?}");
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
        for bad in ["", "1", "1.0", "1.0.0.0", "1.0.x", "65536.0.0", "1..0", "v1.0.0"] {
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
    fn file_associations_may_be_empty_but_must_be_well_formed_and_unique() {
        let mut spec = valid();
        spec.file_associations = vec![];
        assert_eq!(spec.validate_fields(), Ok(()));

        for bad_extension in ["", "psd", ".", ".PSD", ".ps d", &format!(".{}", "a".repeat(17))] {
            let mut spec = valid();
            spec.file_associations[0].extension = bad_extension.into();
            assert_eq!(field_of(spec.validate_fields()), "fileAssociations");
        }
        for bad_mime in ["", "image", "image/", "/photoshop", "image / x", "image/x/y"] {
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
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p webwrap-spec schema`
Expected: FAIL — `cannot find struct AppSpec`.

- [ ] **Step 3: Implement the types and field validation**

Prepend to `crates/webwrap-spec/src/schema.rs`:

```rust
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use url::Url;

/// Largest accepted specification resource. A specialized executable carries
/// a handful of short strings; anything approaching this is a defect.
pub const MAX_SPEC_BYTES: usize = 64 * 1024;

/// Names Windows reserves device-wide. The slug becomes a filename, so a slug
/// of "nul" would ask the generator to publish `nul.exe`.
const RESERVED_DEVICE_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
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
    TooLarge { bytes: usize },
    NotUtf8,
    Json(String),
    Field { field: &'static str, reason: String },
    NotCanonical { launch_url: String, canonical: String },
    IdentityMismatch { app_id: String, expected: String },
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { bytes } => write!(
                f,
                "the embedded configuration is {bytes} bytes, which exceeds the {MAX_SPEC_BYTES} byte limit"
            ),
            Self::NotUtf8 => f.write_str("the embedded configuration is not valid UTF-8"),
            Self::Json(message) => write!(f, "the embedded configuration is not valid JSON: {message}"),
            Self::Field { field, reason } => write!(f, "invalid {field}: {reason}"),
            Self::NotCanonical { launch_url, canonical } => write!(
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

/// Shared rule for human-readable single-line strings.
fn check_text(value: &str, name: &'static str) -> Result<(), SpecError> {
    let length = value.chars().count();
    if length == 0 {
        return Err(field(name, "must not be empty"));
    }
    if length > 255 {
        return Err(field(name, "must be at most 255 characters"));
    }
    if value.chars().any(char::is_control) {
        return Err(field(name, "must not contain control characters"));
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
    if !rest.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
        return Err(format!(
            "extension {extension} must use only lowercase letters and digits"
        ));
    }
    let mime = &assoc.mime_type;
    let parts: Vec<&str> = mime.split('/').collect();
    if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
        return Err(format!("mimeType {mime} must be type/subtype"));
    }
    if mime.chars().any(|c| !c.is_ascii_graphic()) {
        return Err(format!("mimeType {mime} must not contain spaces"));
    }
    if assoc.description.is_empty()
        || assoc.description.chars().count() > 255
        || assoc.description.chars().any(char::is_control)
    {
        return Err("description must be 1 to 255 characters with no control characters".into());
    }
    Ok(())
}

impl AppSpec {
    /// Check every field in isolation. Cross-field invariants live in
    /// `validate`, added in the next task.
    pub fn validate_fields(&self) -> Result<(), SpecError> {
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

        check_text(&self.display_name, "displayName")?;

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

        let components: Vec<&str> = self.app_version.split('.').collect();
        if components.len() != 3
            || components
                .iter()
                .any(|part| part.parse::<u16>().is_err())
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
}
```

`crates/webwrap-spec/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod identity;
pub mod policy;
pub mod schema;

pub use identity::{app_identity, canonicalize_url, IdentityError};
pub use policy::{is_trusted_origin, nav_verdict, NavEvent, Verdict};
pub use schema::{Adapter, AppSpec, Assoc, DnsMode, SpecError, MAX_SPEC_BYTES};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p webwrap-spec`
Expected: PASS, 21 tests (16 unit tests plus the 5 contract vectors).

Note on `appVersion`: `"1.0.0".parse::<u16>()` rejects a leading `+` and a leading `-`, so `"+1.0.0"` and `"-1.0.0"` fail as required. `"01.0.0"` is accepted; leading zeros are harmless because the value is written into a numeric PE `VERSIONINFO` field.

- [ ] **Step 5: Run the quality gates**

Run: `cargo clippy -p webwrap-spec --all-targets -- -D warnings`
Run: `cargo fmt --all -- --check`

- [ ] **Step 6: Commit**

```bash
git add crates/webwrap-spec/src/schema.rs crates/webwrap-spec/src/lib.rs
git commit -m "feat(spec): define the app specification and validate every field"
```

---

### Task 4: Structural invariants and the parse entry point

**Files:**
- Modify: `crates/webwrap-spec/src/schema.rs`

**Interfaces:**
- Consumes: `canonicalize_url`, `app_identity` from Task 1; `validate_fields`, `SpecError`, `MAX_SPEC_BYTES` from Task 3.
- Produces: `AppSpec::validate(&self) -> Result<(), SpecError>`; `AppSpec::from_slice(bytes: &[u8]) -> Result<AppSpec, SpecError>`.

- [ ] **Step 1: Write the failing test**

Append to the `tests` module in `crates/webwrap-spec/src/schema.rs`:

```rust
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
        assert!(matches!(spec.validate(), Err(SpecError::Field { field: "launchUrl", .. })));
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p webwrap-spec schema`
Expected: FAIL — `no method named validate` / `no function or associated item named from_slice`.

- [ ] **Step 3: Implement the invariants and entry point**

Add to the `impl AppSpec` block in `crates/webwrap-spec/src/schema.rs`, and add `use crate::identity::{app_identity, canonicalize_url};` to the module's imports:

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p webwrap-spec`
Expected: PASS, 27 tests (22 unit tests plus the 5 contract vectors).

- [ ] **Step 5: Run the quality gates**

Run: `cargo clippy -p webwrap-spec --all-targets -- -D warnings`
Run: `cargo fmt --all -- --check`
Run: `git diff --check`

- [ ] **Step 6: Commit**

```bash
git add crates/webwrap-spec/src/schema.rs
git commit -m "feat(spec): enforce canonical launch URL and matching identity"
```

**Sub-project A is complete at this commit.** Everything from here needs Windows.

---

### Task 5: Runtime scaffold that the specializer can edit

**Files:**
- Create: `crates/webwrap-runtime/Cargo.toml`
- Create: `crates/webwrap-runtime/build.rs`
- Create: `crates/webwrap-runtime/tauri.conf.json`
- Create: `crates/webwrap-runtime/ui/index.html`
- Create: `crates/webwrap-runtime/src/main.rs`
- Test: `crates/webwrap-runtime/tests/inspect_self.rs`

**Interfaces:**
- Consumes: `webwrap-spec` by path.
- Produces: a `webwrap-runtime.exe` binary for both target triples that carries a main icon and a `VERSIONINFO` resource, so the existing `webwrap-pe-specialize` spike binary can specialize it. Also produces the test helper `fn specialize_fixture(spec_json: &str, display_name: &str, version: &str) -> PathBuf` used by Tasks 6-8.

- [ ] **Step 0: Install both Windows targets for the default toolchain**

```bash
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
rustup target list --installed
```

Expected: both triples listed. This is needed once per machine, and it is why `rust-toolchain.toml` carries no `targets` key — that key would impose the same download on the WSL toolchain, which never builds for Windows.

- [ ] **Step 1: Create the crate**

`crates/webwrap-runtime/Cargo.toml`:

```toml
[package]
name = "webwrap-runtime"
version = "0.1.0"
edition = "2021"
build = "build.rs"

[build-dependencies]
tauri-build = "=2.6.3"

[dependencies]
serde_json = "=1.0.151"
tauri = "=2.11.5"
webwrap-spec = { path = "../webwrap-spec" }

[target.'cfg(windows)'.dependencies]
windows = { version = "=0.61.3", features = [
    "Win32_Foundation",
    "Win32_System_LibraryLoader",
    "Win32_UI_WindowsAndMessaging",
] }

[[bin]]
name = "webwrap-runtime"
path = "src/main.rs"
```

`crates/webwrap-runtime/build.rs` — the placeholder icon is what makes the template specializable. `editpe`'s `remove_main_icon` then `set_main_icon_file` sequence needs a main icon to exist first. This 1x1 transparent ICO trick is copied from `spikes/photopea-windows-writeback/build.rs`, where it is already proven:

```rust
use std::{env, fs, path::PathBuf};

fn main() {
    let icon = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("icon.ico");
    // 1x1 transparent 32-bit DIB wrapped in the minimal ICO container. This is
    // a placeholder: the generator replaces the main icon during
    // specialization. It exists so that replacement has something to replace.
    fs::write(
        &icon,
        [
            0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 32, 0, 48, 0, 0, 0, 22, 0, 0, 0, 40, 0, 0, 0, 1, 0,
            0, 0, 2, 0, 0, 0, 1, 0, 32, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    )
    .unwrap();
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new().window_icon_path(icon)),
    )
    .unwrap();
}
```

`crates/webwrap-runtime/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "WebWrap Runtime",
  "version": "0.1.0",
  "identifier": "dev.webwrap.runtime",
  "build": { "frontendDist": "ui" },
  "app": { "withGlobalTauri": false, "windows": [] },
  "bundle": { "active": false }
}
```

`crates/webwrap-runtime/ui/index.html` — Tauri requires a local asset root even though the runtime loads a remote URL:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>WebWrap</title>
  </head>
  <body></body>
</html>
```

`crates/webwrap-runtime/src/main.rs` — a stub that proves the crate builds and carries resources. Task 6 replaces the body:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("webwrap-runtime must be built and run on Windows");

fn main() {
    // Replaced in Task 6. Exists so the template can be built and specialized.
    std::process::exit(0);
}
```

- [ ] **Step 2: Build for both targets to verify the template compiles**

```bash
cargo build -p webwrap-runtime --target x86_64-pc-windows-msvc
cargo build -p webwrap-runtime --target aarch64-pc-windows-msvc
```

Expected: both succeed, producing `target/<triple>/debug/webwrap-runtime.exe`.

- [ ] **Step 3: Write the failing test that the template is specializable**

`crates/webwrap-runtime/tests/inspect_self.rs`:

```rust
//! Native integration tests. These need the `webwrap-pe-specialize` binary
//! from `spikes/windows-pe-specialization`, which is still in the workspace
//! precisely because spike removal is staged per porter. Sub-project C
//! replaces `specializer_path` with the real generator and nothing else here
//! changes.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// A specification whose appId matches its launchUrl, as the invariant
/// requires. The identity of `https://www.photopea.com/` is
/// `873756fbe97a1384`, pinned by `webwrap-spec`'s contract vectors.
const PHOTOPEA_SPEC: &str = r#"{
  "schemaVersion": 1,
  "appId": "873756fbe97a1384",
  "displayName": "Photopea",
  "slug": "photopea",
  "launchUrl": "https://www.photopea.com/",
  "trustedOrigins": ["https://www.photopea.com"],
  "adapter": "photopea",
  "fileAssociations": [
    {
      "extension": ".psd",
      "mimeType": "image/vnd.adobe.photoshop",
      "description": "Photoshop document"
    }
  ],
  "dnsMode": "system",
  "appVersion": "1.0.0"
}"#;

/// The identity of `https://twitter.com/` is `2fa5e3f40150278f`.
const TWITTER_SPEC: &str = r#"{
  "schemaVersion": 1,
  "appId": "2fa5e3f40150278f",
  "displayName": "Twitter",
  "slug": "twitter",
  "launchUrl": "https://twitter.com/",
  "trustedOrigins": ["https://twitter.com"],
  "adapter": "none",
  "fileAssociations": [],
  "dnsMode": "system",
  "appVersion": "1.0.0"
}"#;

fn unique(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "webwrap-runtime-{label}-{}-{nonce}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Directory holding the test binary, which is also where cargo puts the
/// built `webwrap-runtime.exe` and `webwrap-pe-specialize.exe`.
fn build_dir() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // the deps/ directory
    if path.ends_with("deps") {
        path.pop();
    }
    path
}

fn template_path() -> PathBuf {
    let path = build_dir().join("webwrap-runtime.exe");
    assert!(
        path.is_file(),
        "template not built at {}; run cargo build -p webwrap-runtime --target <triple>",
        path.display()
    );
    path
}

fn specializer_path() -> PathBuf {
    let path = build_dir().join("webwrap-pe-specialize.exe");
    assert!(
        path.is_file(),
        "specializer not built at {}; run cargo build -p windows-pe-specialization-spike --target <triple>",
        path.display()
    );
    path
}

/// A minimal valid PNG for the icon argument: 1x1 opaque black.
const ICON_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd, 0x8d, 0xb0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

/// Specialize the built template with the given specification and return the
/// path of the produced executable.
pub fn specialize_fixture(spec_json: &str, display_name: &str, version: &str) -> PathBuf {
    let dir = unique("case");
    fs::create_dir_all(&dir).unwrap();
    let spec = dir.join("spec.json");
    fs::write(&spec, spec_json).unwrap();
    let icon = dir.join("icon.png");
    fs::write(&icon, ICON_PNG).unwrap();
    let output = dir.join("app.exe");

    let status = Command::new(specializer_path())
        .args([
            "specialize",
            "--input",
            template_path().to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--spec",
            spec.to_str().unwrap(),
            "--icon",
            icon.to_str().unwrap(),
            "--display-name",
            display_name,
            "--version",
            version,
        ])
        .status()
        .unwrap();
    assert!(status.success(), "specialize failed with {status:?}");
    assert!(output.is_file());
    output
}

fn inspect_with_specializer(exe: &Path) -> serde_json::Value {
    let out = Command::new(specializer_path())
        .args(["inspect", "--input", exe.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "inspect failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn the_template_carries_the_resources_specialization_needs() {
    let exe = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    let report = inspect_with_specializer(&exe);

    assert_eq!(report["specBytes"].as_u64().unwrap() as usize, PHOTOPEA_SPEC.len());
    assert_eq!(report["iconPresent"], serde_json::json!(true));
    // Proves the version strings were actually replaced rather than leaving a
    // stale value from the template's own VERSIONINFO.
    assert_eq!(report["fileDescription"], serde_json::json!("Photopea"));
    assert_eq!(report["productVersion"], serde_json::json!("1.0.0"));
}
```

- [ ] **Step 4: Run the test to verify it fails, then passes**

```bash
cargo build -p windows-pe-specialization-spike --target x86_64-pc-windows-msvc
cargo test -p webwrap-runtime --target x86_64-pc-windows-msvc
```

Expected on a first run before the template exists: FAIL with the "template not built" assertion. After Step 2's build: PASS, 1 test.

If `fileDescription` comes back as `"WebWrap Runtime"` instead of `"Photopea"`, the template has more than one `VersionStringTable` and only `strings[0]` is being updated — the known limitation recorded as a `ponytail:` comment in `spikes/windows-pe-specialization/src/pe.rs:98`. Report this rather than working around it; it changes C's design.

- [ ] **Step 5: Commit**

```bash
git add crates/webwrap-runtime
git commit -m "feat(runtime): add the specializable Windows wrapper template"
```

---

### Task 6: Read the embedded specification and fail visibly

**Files:**
- Create: `crates/webwrap-runtime/src/embedded_spec.rs`
- Create: `crates/webwrap-runtime/src/fatal.rs`
- Create: `crates/webwrap-runtime/src/inspect.rs`
- Modify: `crates/webwrap-runtime/src/main.rs`
- Modify: `crates/webwrap-runtime/tests/inspect_self.rs`

**Interfaces:**
- Consumes: `AppSpec::from_slice` from Task 4; `specialize_fixture` from Task 5.
- Produces: `embedded_spec::read() -> Result<Vec<u8>, String>`; `fatal::abort(message: &str, interactive: bool) -> !`; `inspect::run(spec: &AppSpec, data_dir: &Path, check_urls: &[String]) -> !`; `main::parse_args` returning `Mode::Run` or `Mode::InspectSelf { check_urls }`.

- [ ] **Step 1: Write the failing test**

Append to `crates/webwrap-runtime/tests/inspect_self.rs`:

```rust
fn inspect_self(exe: &Path, check_urls: &[&str]) -> serde_json::Value {
    let mut command = Command::new(exe);
    command.arg("--inspect-self");
    for url in check_urls {
        command.args(["--check-url", url]);
    }
    let out = command.output().unwrap();
    assert!(
        out.status.success(),
        "--inspect-self failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn a_specialized_executable_reports_its_own_specification() {
    let exe = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    let report = inspect_self(&exe, &[]);

    assert_eq!(report["schemaVersion"], serde_json::json!(1));
    assert_eq!(report["appId"], serde_json::json!("873756fbe97a1384"));
    assert_eq!(report["displayName"], serde_json::json!("Photopea"));
    assert_eq!(report["slug"], serde_json::json!("photopea"));
    assert_eq!(
        report["launchUrl"],
        serde_json::json!("https://www.photopea.com/")
    );
    assert_eq!(
        report["trustedOrigins"],
        serde_json::json!(["https://www.photopea.com"])
    );
    assert_eq!(report["adapter"], serde_json::json!("photopea"));
    assert_eq!(report["dnsMode"], serde_json::json!("system"));
    assert_eq!(report["appVersion"], serde_json::json!("1.0.0"));
    assert_eq!(
        report["fileAssociations"][0]["extension"],
        serde_json::json!(".psd")
    );
    // Derived from the running binary, never stored in the specification, so
    // it cannot disagree with the actual executable.
    let arch = report["runtimeArch"].as_str().unwrap();
    assert!(
        arch == "x86_64" || arch == "aarch64",
        "unexpected arch {arch}"
    );
}

#[test]
fn renaming_and_moving_the_executable_does_not_break_the_read() {
    let exe = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    let moved = unique("moved");
    fs::create_dir_all(&moved).unwrap();
    let renamed = moved.join("totally-different-name.exe");
    fs::rename(&exe, &renamed).unwrap();

    // The resource is read from the loaded image, not by reopening the file
    // by its original path.
    let report = inspect_self(&renamed, &[]);
    assert_eq!(report["appId"], serde_json::json!("873756fbe97a1384"));
}

#[test]
fn an_unspecialized_template_refuses_to_run() {
    let out = Command::new(template_path())
        .arg("--inspect-self")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("never specialized"),
        "unhelpful message: {stderr}"
    );
}

#[test]
fn unknown_arguments_are_a_usage_error() {
    let exe = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    for args in [
        vec!["--nonsense"],
        vec!["some-file.psd"],
        vec!["--inspect-self", "--check-url"],
        vec!["--inspect-self", "--check-url", "--nonsense"],
        vec!["--inspect-self", "extra"],
    ] {
        let out = Command::new(&exe).args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "args {args:?} should be usage errors");
    }
}

#[test]
fn a_specification_that_fails_validation_is_rejected_at_startup() {
    // appId does not match launchUrl, so invariant 2 must reject it. The
    // specializer's own weaker validator accepts this, which is exactly why
    // the runtime revalidates.
    let mismatched = PHOTOPEA_SPEC.replace("873756fbe97a1384", "0000000000000000");
    let exe = specialize_fixture(&mismatched, "Photopea", "1.0.0");
    let out = Command::new(&exe).arg("--inspect-self").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("appId"), "unhelpful message: {stderr}");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p webwrap-runtime --target x86_64-pc-windows-msvc`
Expected: FAIL — the stub `main` exits 0 and prints nothing, so JSON parsing fails.

- [ ] **Step 3: Implement the resource read**

`crates/webwrap-runtime/src/embedded_spec.rs`:

```rust
use std::slice;

use windows::core::PCWSTR;
use windows::Win32::System::LibraryLoader::{
    FindResourceExW, GetModuleHandleW, LoadResource, LockResource, SizeofResource,
};

/// Resource type RT_RCDATA, expressed the way Win32 wants it: a small integer
/// smuggled through a string pointer.
const RT_RCDATA: PCWSTR = PCWSTR(10 as *const u16);
/// The resource name and language the generator writes. Fixed by
/// `spikes/windows-pe-specialization/src/pe.rs`.
const RESOURCE_NAME: &str = "WEBWRAP_APP_SPEC";
const LANGUAGE_EN_US: u16 = 0x0409;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

/// Read the embedded specification from this process's own loaded image.
///
/// Reading the loaded image rather than reopening the executable file keeps
/// renaming, moving, and file locking harmless, and needs no file I/O at
/// startup. The language is requested explicitly so the lookup does not
/// depend on the machine's locale.
pub fn read() -> Result<Vec<u8>, String> {
    let name = wide(RESOURCE_NAME);
    unsafe {
        let module =
            GetModuleHandleW(PCWSTR::null()).map_err(|error| format!("module handle: {error}"))?;
        let found = FindResourceExW(
            Some(module.into()),
            RT_RCDATA,
            PCWSTR(name.as_ptr()),
            LANGUAGE_EN_US,
        );
        if found.0.is_null() {
            return Err(
                "this application has no embedded configuration, so it was never specialized by the generator"
                    .into(),
            );
        }
        let size = SizeofResource(Some(module.into()), found);
        if size == 0 {
            return Err("the embedded configuration is empty".into());
        }
        let handle = LoadResource(Some(module.into()), found)
            .map_err(|error| format!("load resource: {error}"))?;
        let pointer = LockResource(handle);
        if pointer.is_null() {
            return Err("the embedded configuration could not be read".into());
        }
        Ok(slice::from_raw_parts(pointer as *const u8, size as usize).to_vec())
    }
}
```

**API check, not a guess:** `windows` 0.61 changed several of these signatures to take `Option<HMODULE>` and to return `Result`. Verify each of `GetModuleHandleW`, `FindResourceExW`, `SizeofResource`, `LoadResource`, `LockResource` against the 0.61.3 docs and adjust the `Option` wrapping, the `HRSRC` emptiness check (`found.0.is_null()` versus `found.is_invalid()`), and the `Result` handling to match. The logic above is complete; only the exact call shapes may differ. Record what you found in the evidence document.

`crates/webwrap-runtime/src/fatal.rs`:

```rust
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

/// Report an unrecoverable startup failure and exit.
///
/// A release build sets `windows_subsystem = "windows"`, so it has no console
/// and `eprintln!` reaches nobody. Interactive failures therefore go through
/// a message box. Non-interactive modes keep writing to stderr so that tests
/// and pipelines can read the reason.
pub fn abort(message: &str, interactive: bool) -> ! {
    if interactive {
        let title = HSTRING::from("WebWrap");
        let body = HSTRING::from(message);
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(body.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    } else {
        eprintln!("{message}");
    }
    std::process::exit(1)
}
```

`crates/webwrap-runtime/src/inspect.rs`:

```rust
use std::path::Path;

use serde_json::json;
use webwrap_spec::AppSpec;

/// Print the runtime's own resolved configuration as one JSON object.
///
/// This is what makes the runtime's behaviour assertable by automated tests
/// instead of eyeballed. `navVerdicts` is filled in by Task 8.
pub fn run(spec: &AppSpec, data_dir: &Path, _check_urls: &[String]) -> ! {
    let report = json!({
        "schemaVersion": spec.schema_version,
        "appId": spec.app_id,
        "displayName": spec.display_name,
        "slug": spec.slug,
        "launchUrl": spec.launch_url,
        "trustedOrigins": spec.trusted_origins,
        "adapter": spec.adapter,
        "fileAssociations": spec.file_associations,
        "dnsMode": spec.dns_mode,
        "appVersion": spec.app_version,
        "runtimeArch": std::env::consts::ARCH,
        "webview2DataDir": data_dir.to_string_lossy(),
        "navVerdicts": [],
    });
    println!("{report}");
    std::process::exit(0)
}
```

`crates/webwrap-runtime/src/main.rs`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("webwrap-runtime must be built and run on Windows");

mod embedded_spec;
mod fatal;
mod inspect;

use std::path::PathBuf;

use webwrap_spec::AppSpec;

enum Mode {
    Run,
    InspectSelf { check_urls: Vec<String> },
}

/// Parse arguments.
///
/// A positional file path is rejected for now. Sub-project D adds the file
/// bridge and relaxes this; accepting a path before anything can open it
/// would look like success and do nothing.
fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Mode, String> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Mode::Run);
    };
    if first != "--inspect-self" {
        return Err(format!("unknown argument: {first}"));
    }
    let mut check_urls = Vec::new();
    while let Some(flag) = args.next() {
        if flag != "--check-url" {
            return Err(format!("unknown argument: {flag}"));
        }
        let value = args
            .next()
            .ok_or_else(|| "missing value for --check-url".to_string())?;
        if value.starts_with("--") {
            return Err("missing value for --check-url".into());
        }
        check_urls.push(value);
    }
    Ok(Mode::InspectSelf { check_urls })
}

/// Storage location for this app identity.
///
/// Every generated executable shares one compiled runtime, so on Windows
/// isolation has to be explicit; on Android a unique application ID would
/// provide it for free.
fn data_dir(spec: &AppSpec) -> Result<PathBuf, String> {
    let local = std::env::var("LOCALAPPDATA").map_err(|_| {
        "LOCALAPPDATA is not set, so this app cannot create its own isolated storage".to_string()
    })?;
    if local.is_empty() {
        return Err(
            "LOCALAPPDATA is empty, so this app cannot create its own isolated storage".into(),
        );
    }
    Ok(PathBuf::from(local).join("WebWrap").join(&spec.app_id))
}

fn main() {
    let mode = match parse_args(std::env::args().skip(1)) {
        Ok(mode) => mode,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let interactive = matches!(mode, Mode::Run);

    let bytes = match embedded_spec::read() {
        Ok(bytes) => bytes,
        Err(error) => fatal::abort(&error, interactive),
    };
    let spec = match AppSpec::from_slice(&bytes) {
        Ok(spec) => spec,
        Err(error) => fatal::abort(&error.to_string(), interactive),
    };
    let data_dir = match data_dir(&spec) {
        Ok(path) => path,
        Err(error) => fatal::abort(&error, interactive),
    };

    match mode {
        Mode::InspectSelf { check_urls } => inspect::run(&spec, &data_dir, &check_urls),
        Mode::Run => {
            // Task 8 opens the window here.
            std::process::exit(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_arguments_means_run() {
        assert!(matches!(
            parse_args(Vec::<String>::new()).unwrap(),
            Mode::Run
        ));
    }

    #[test]
    fn inspect_self_collects_repeated_check_urls() {
        let mode = parse_args(
            ["--inspect-self", "--check-url", "https://a.test/", "--check-url", "http://b.test/"]
                .map(String::from),
        )
        .unwrap();
        match mode {
            Mode::InspectSelf { check_urls } => {
                assert_eq!(check_urls, ["https://a.test/", "http://b.test/"]);
            }
            Mode::Run => panic!("expected inspect mode"),
        }
    }

    #[test]
    fn malformed_argument_shapes_are_rejected() {
        for args in [
            vec!["--nonsense"],
            vec!["file.psd"],
            vec!["--inspect-self", "--check-url"],
            vec!["--inspect-self", "--check-url", "--nonsense"],
            vec!["--inspect-self", "extra"],
        ] {
            assert!(
                parse_args(args.iter().map(|a| a.to_string())).is_err(),
                "{args:?}"
            );
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo build -p webwrap-runtime --target x86_64-pc-windows-msvc
cargo test -p webwrap-runtime --target x86_64-pc-windows-msvc
```

Expected: PASS, 3 unit tests plus 6 integration tests.

- [ ] **Step 5: Commit**

```bash
git add crates/webwrap-runtime
git commit -m "feat(runtime): read and validate the embedded specification"
```

---

### Task 7: Per-identity WebView2 storage isolation

**Files:**
- Modify: `crates/webwrap-runtime/tests/inspect_self.rs`
- Modify: `crates/webwrap-runtime/src/main.rs` if the API check below requires it

**Interfaces:**
- Consumes: `data_dir` and `inspect::run` from Task 6.
- Produces: the verified `webview2DataDir` contract, and the recorded decision about how the directory is handed to WebView2.

- [ ] **Step 1: Write the failing test — this is release gate 11**

Append to `crates/webwrap-runtime/tests/inspect_self.rs`:

```rust
#[test]
fn gate_11_two_apps_use_separate_webview2_data_directories() {
    let photopea = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    let twitter = specialize_fixture(TWITTER_SPEC, "Twitter", "1.0.0");

    let a = inspect_self(&photopea, &[]);
    let b = inspect_self(&twitter, &[]);

    let dir_a = a["webview2DataDir"].as_str().unwrap();
    let dir_b = b["webview2DataDir"].as_str().unwrap();

    assert_ne!(dir_a, dir_b, "generated apps must not share storage");
    assert!(dir_a.ends_with("873756fbe97a1384"), "{dir_a}");
    assert!(dir_b.ends_with("2fa5e3f40150278f"), "{dir_b}");
    assert!(dir_a.contains("WebWrap"), "{dir_a}");

    // The directory is derived from the identity, and the identity is pinned
    // to the launch URL by the specification's own invariant, so storage
    // cannot drift away from the site it belongs to.
    assert_eq!(a["appId"], serde_json::json!("873756fbe97a1384"));
    assert_eq!(b["appId"], serde_json::json!("2fa5e3f40150278f"));
}

#[test]
fn the_data_directory_sits_under_localappdata() {
    let exe = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    let report = inspect_self(&exe, &[]);
    let local = std::env::var("LOCALAPPDATA").unwrap();
    let dir = report["webview2DataDir"].as_str().unwrap();
    assert!(dir.starts_with(&local), "{dir} is not under {local}");
}
```

- [ ] **Step 2: Run the test to verify it passes or fails**

Run: `cargo test -p webwrap-runtime --target x86_64-pc-windows-msvc gate_11`
Expected: PASS, because Task 6 already computes the directory. If it fails, the derivation in `data_dir` is wrong — fix that, not the test.

- [ ] **Step 3: Resolve how the directory reaches WebView2**

This is the one API question the spec flags. Determine, against `tauri` 2.11.5 as pinned, which of these exists:

1. An explicit builder method on `WebviewWindowBuilder` or on the Tauri/wry Windows extension trait that sets the WebView2 user data folder.
2. The `WEBVIEW2_USER_DATA_FOLDER` environment variable read by the WebView2 loader.

**Prefer option 1.** An environment variable is inherited by every child process the app spawns, which silently widens its effect. Use option 2 only if no builder method exists on the pinned version.

Search order: `cargo doc -p tauri --open` and look for `data_directory` on the webview builder; then the `tauri::webview::WebviewBuilder` and wry `WebViewBuilderExtWindows` docs; then the WebView2 user-data-folder reference linked from the spec.

Record in the evidence document: which mechanism exists, which you used, and why.

- [ ] **Step 4: Apply the chosen mechanism**

If option 1 exists, hold the resolved `data_dir` and pass it to the builder in Task 8's window construction. If only option 2 exists, set it in `main` immediately before the `Mode::Run` branch — never earlier, so `--inspect-self` does not mutate the environment:

```rust
        Mode::Run => {
            // ponytail: environment variable rather than a builder method,
            // because tauri 2.11.5 exposes no data-directory setter. Revisit
            // when it does; an env var is inherited by child processes.
            std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &data_dir);
            // Task 8 opens the window here.
            std::process::exit(0)
        }
```

- [ ] **Step 5: Run the full suite and commit**

```bash
cargo test -p webwrap-runtime --target x86_64-pc-windows-msvc
git add crates/webwrap-runtime
git commit -m "feat(runtime): isolate WebView2 storage per app identity"
```

---

### Task 8: Open the window and enforce navigation policy

**Files:**
- Modify: `crates/webwrap-runtime/src/main.rs`
- Modify: `crates/webwrap-runtime/src/inspect.rs`
- Modify: `crates/webwrap-runtime/tests/inspect_self.rs`

**Interfaces:**
- Consumes: `nav_verdict`, `NavEvent`, `Verdict` from Task 2; `data_dir` from Task 6.
- Produces: a runtime that opens `spec.launch_url` titled `spec.display_name` with navigation policy attached, and a populated `navVerdicts` array.

- [ ] **Step 1: Write the failing test — this is release gate 14**

Append to `crates/webwrap-runtime/tests/inspect_self.rs`:

```rust
#[test]
fn gate_14_navigation_policy_is_enforced_by_the_real_binary() {
    let exe = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    let report = inspect_self(
        &exe,
        &[
            "https://www.photopea.com/",
            "https://accounts.google.com/o/oauth2/auth",
            "http://www.photopea.com/",
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)",
            "not a url",
        ],
    );

    let verdicts = report["navVerdicts"].as_array().unwrap();
    assert_eq!(verdicts.len(), 6);

    let verdict_for = |url: &str| -> String {
        verdicts
            .iter()
            .find(|entry| entry["url"] == serde_json::json!(url))
            .unwrap_or_else(|| panic!("no verdict for {url}"))["verdict"]
            .as_str()
            .unwrap()
            .to_string()
    };

    assert_eq!(verdict_for("https://www.photopea.com/"), "allow");
    // An untrusted https origin is still allowed to navigate, or login breaks.
    assert_eq!(
        verdict_for("https://accounts.google.com/o/oauth2/auth"),
        "allow"
    );
    assert_eq!(verdict_for("http://www.photopea.com/"), "block");
    assert_eq!(verdict_for("file:///C:/Windows/win.ini"), "block");
    assert_eq!(verdict_for("javascript:alert(1)"), "block");
    assert_eq!(verdict_for("not a url"), "block");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p webwrap-runtime --target x86_64-pc-windows-msvc gate_14`
Expected: FAIL — `navVerdicts` is an empty array, so `verdicts.len()` is 0, not 6.

- [ ] **Step 3: Populate the verdicts**

Replace the body of `crates/webwrap-runtime/src/inspect.rs`:

```rust
use std::path::Path;

use serde_json::{json, Value};
use webwrap_spec::{nav_verdict, AppSpec, NavEvent, Verdict};

fn verdict_name(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Allow => "allow",
        Verdict::Block => "block",
        Verdict::OpenExternally => "openExternally",
    }
}

/// Print the runtime's own resolved configuration as one JSON object.
///
/// Each `--check-url` is evaluated as a top-level navigation. The new-window
/// verdict is constant — every pop-up opens in the system browser — so it
/// needs no per-URL probe and is covered by `webwrap-spec`'s unit tests.
pub fn run(spec: &AppSpec, data_dir: &Path, check_urls: &[String]) -> ! {
    let verdicts: Vec<Value> = check_urls
        .iter()
        .map(|url| {
            json!({
                "url": url,
                "verdict": verdict_name(nav_verdict(url, NavEvent::TopLevel)),
            })
        })
        .collect();
    let report = json!({
        "schemaVersion": spec.schema_version,
        "appId": spec.app_id,
        "displayName": spec.display_name,
        "slug": spec.slug,
        "launchUrl": spec.launch_url,
        "trustedOrigins": spec.trusted_origins,
        "adapter": spec.adapter,
        "fileAssociations": spec.file_associations,
        "dnsMode": spec.dns_mode,
        "appVersion": spec.app_version,
        "runtimeArch": std::env::consts::ARCH,
        "webview2DataDir": data_dir.to_string_lossy(),
        "navVerdicts": verdicts,
    });
    println!("{report}");
    std::process::exit(0)
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p webwrap-runtime --target x86_64-pc-windows-msvc gate_14`
Expected: PASS.

- [ ] **Step 5: Open the actual window**

Replace the `Mode::Run` branch in `crates/webwrap-runtime/src/main.rs` with a call to `run`, and add:

```rust
use tauri::{WebviewUrl, WebviewWindowBuilder};
use webwrap_spec::{nav_verdict, NavEvent};

fn run(spec: AppSpec, data_dir: PathBuf) -> ! {
    // Validated as an https URL by AppSpec::validate, so parsing cannot fail.
    let url: tauri::Url = spec
        .launch_url
        .parse()
        .expect("launchUrl was validated as an https URL");
    let title = spec.display_name.clone();
    let _ = &data_dir; // consumed by the mechanism chosen in Task 7

    tauri::Builder::default()
        .setup(move |app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.clone()))
                .title(title.clone())
                .on_navigation(|url| nav_verdict(url.as_str(), NavEvent::TopLevel).is_allow())
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start the WebWrap runtime");
    std::process::exit(0)
}
```

If Task 7 chose the builder method, pass `data_dir` to it here instead of the `let _ =` line and delete that line.

**New-window handling:** the constant `OpenExternally` verdict needs a handler that hands the URL to the system browser. Check whether `tauri` 2.11.5 exposes a new-window hook on `WebviewWindowBuilder`. If it does, wire it to `tauri_plugin_opener` or `ShellExecuteW`. If it does **not**, leave it unwired and record that in the evidence document: the policy is defined and unit-tested, and sub-project D already reaches the platform webview through `with_webview` for the file bridge, which is the natural place to install the handler. Do not add a new dependency for this in sub-project B.

- [ ] **Step 6: Build both targets and run the full suite**

```bash
cargo build -p webwrap-runtime --target x86_64-pc-windows-msvc
cargo build -p webwrap-runtime --target aarch64-pc-windows-msvc
cargo test -p webwrap-runtime --target x86_64-pc-windows-msvc
cargo test -p webwrap-runtime --target aarch64-pc-windows-msvc
```

Expected: all pass. The aarch64 run asserts `runtimeArch` is `aarch64`; the x64 run asserts `x86_64`. Both are accepted by the assertion written in Task 6.

- [ ] **Step 7: Commit**

```bash
git add crates/webwrap-runtime
git commit -m "feat(runtime): open the embedded launch URL under navigation policy"
```

---

### Task 9: Quality gates and acceptance evidence

**Files:**
- Create: `docs/evidence/2026-09-16-webwrap-spec-and-minimal-runtime.md`
- Modify: `HANDOVER.md`

**Interfaces:**
- Consumes: everything from Tasks 1-8.
- Produces: the committed evidence document and an updated handover pointing at sub-project C.

- [ ] **Step 1: Run every quality gate**

```bash
cargo test -p webwrap-spec
cargo clippy -p webwrap-spec --all-targets -- -D warnings
cargo clippy -p webwrap-runtime --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cargo clippy -p webwrap-runtime --all-targets --target aarch64-pc-windows-msvc -- -D warnings
cargo test -p webwrap-runtime --target x86_64-pc-windows-msvc
cargo test -p webwrap-runtime --target aarch64-pc-windows-msvc
cargo fmt --all -- --check
git diff --check
```

Expected: every command passes. Record the exact test counts.

- [ ] **Step 2: Run the manual GUI acceptance check**

Automated tests cover configuration and policy but never open a window. Run once per architecture:

1. Specialize the template into a Photopea app using `webwrap-pe-specialize` with a real 512x512 PNG icon.
2. Launch the produced executable with no arguments.
3. Confirm the window title is `Photopea`, that `photopea.com` loads, and that the taskbar icon is the supplied icon rather than the 1x1 placeholder.
4. Confirm `%LOCALAPPDATA%\WebWrap\873756fbe97a1384\` was created and populated.
5. Specialize a second app from `TWITTER_SPEC`, launch both at once, and confirm two separate directories exist and that signing into one does not affect the other.
6. Launch the unspecialized template directly and confirm a message box appears saying it was never specialized — not a silent exit.

Record the observed results, including the Windows build number and WebView2 runtime version.

- [ ] **Step 3: Write the evidence document**

`docs/evidence/2026-09-16-webwrap-spec-and-minimal-runtime.md` must contain:

- Date, git commit tested, and the statement that this covers sub-projects A and B only.
- Exact environment: `rustc -Vv` output, Windows version, WebView2 runtime version, both target triples.
- Per-suite test counts from Step 1.
- Which WebView2 data-directory mechanism `tauri` 2.11.5 exposes, which was used, and why (from Task 7 Step 3).
- Whether a new-window hook exists on the pinned Tauri version, and if not, that pop-up routing is deferred to sub-project D with the policy already defined and tested.
- The exact `windows` 0.61.3 call shapes used in `embedded_spec.rs`, since these were verified rather than assumed.
- Gate 11 and gate 14 observations from Steps 1 and 2.
- An explicit statement that gates 1, 4, 5, 6, 7, and 8 are **not** proven by this work.

- [ ] **Step 4: Update the handover**

In `HANDOVER.md`, change the A and B rows of the sub-project table to reflect completion, move the "Immediate next step" section to point at sub-project C, and note that C should now brainstorm and spec the packaging core, port `pe.rs` out of the spike, and delete `spikes/windows-pe-specialization` — remembering that `crates/webwrap-runtime/tests/inspect_self.rs` calls `specializer_path()` and must be repointed at the generator in the same change.

- [ ] **Step 5: Commit**

```bash
git add docs/evidence HANDOVER.md
git commit -m "docs: record spec and runtime acceptance evidence"
```

---

## Notes carried forward

- `spikes/windows-pe-specialization/src/pe.rs:98` carries a `ponytail:` comment: only `strings[0]` of the version info is updated, so a base PE with more than one `VersionStringTable` keeps stale values in the rest. Task 5 Step 4 is the first time this runs against a Tauri-built PE rather than the spike's own fixture, so it is the first real test of that assumption.
- The spike's `validate_spec` checks only that `schemaVersion` is an integer and that `appId`, `displayName`, `slug`, and `launchUrl` are non-empty single-line strings. It will happily embed a specification the runtime then rejects. That is intended: the specializer is a fixture tool here, and the runtime is the authority. Task 6's mismatched-identity test depends on exactly this gap.
- The spike's `validate_text` bounds length in **bytes**; `webwrap-spec` bounds `displayName` in **characters**. A display name between 256 bytes and 255 characters therefore passes `webwrap-spec` and is rejected by `--display-name`. Sub-project C owns reconciling this when `pe.rs` is ported.

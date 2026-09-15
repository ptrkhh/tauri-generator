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

/// Whether the input already carries a URL scheme.
///
/// A substring test for `"://"` is not good enough in either direction. It
/// misses a single-slash typo, so `http:/example.com` would be treated as
/// schemeless and become `https://http/example.com` — a fabricated host taken
/// from the scheme text, which then validates and ships as an app that can
/// never resolve. It also fires on a schemeless URL whose query or fragment
/// contains an absolute URL, so `example.com/?next=https://other.test` would
/// be rejected outright, and that is exactly the shape a Photopea or
/// vscode.dev deep link has.
///
/// So look for a scheme only in the part before the first `/`, `?`, or `#`,
/// and treat a purely numeric remainder as a port rather than a scheme so
/// that `twitter.com:8443` still gets the `https://` prefix.
fn has_scheme(value: &str) -> bool {
    let end = value.find(['/', '?', '#']).unwrap_or(value.len());
    let head = &value[..end];
    let Some(colon) = head.find(':') else {
        return false;
    };
    let (scheme, rest) = (&head[..colon], &head[colon + 1..]);
    if !scheme.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return false;
    }
    if !scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return false;
    }
    let rest_is_a_port = !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
    !rest_is_a_port
}

/// Canonicalize a website address into the exact string that identity is
/// derived from.
///
/// A missing scheme becomes `https`. Any scheme that is already present and is
/// not `https` is rejected rather than upgraded: silently rewriting `http://`
/// would change which server the user asked for.
pub fn canonicalize_url(input: &str) -> Result<String, IdentityError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(IdentityError::Unparseable);
    }
    let candidate = if has_scheme(trimmed) {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let url = Url::parse(&candidate).map_err(|_| IdentityError::Unparseable)?;
    if url.scheme() != "https" {
        return Err(IdentityError::NotHttps);
    }
    // Defence in depth rather than a live path: for the special scheme
    // `https` the url crate already rejects an empty host at parse time, and
    // every other scheme was rejected on the line above, so no known input
    // reaches this branch.
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
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

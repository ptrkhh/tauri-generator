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
/// not `https` is rejected rather than upgraded: silently rewriting `http://`
/// would change which server the user asked for.
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
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

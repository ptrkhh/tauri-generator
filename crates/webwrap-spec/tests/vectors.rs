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

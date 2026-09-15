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

/// A substring test for "://" got both of these wrong, and got them wrong
/// silently: a single-slash scheme became a fabricated host taken from the
/// scheme text, and a legitimate deep link was refused. Both are invisible
/// without an explicit case, so these stay as permanent regression vectors.
#[test]
fn a_single_slash_scheme_is_not_mistaken_for_a_missing_scheme() {
    // Each of these once produced a fabricated host, e.g. https://http/...
    for input in [
        "http:/example.com",
        "ftp:/example.com",
        "C:\\Users\\x",
        "D:\\site\\index.html",
        "mailto:a@b.com",
        "javascript:alert(1)",
    ] {
        assert_eq!(
            canonicalize_url(input),
            Err(IdentityError::NotHttps),
            "{input} must be rejected, never given an invented host"
        );
    }

    // The https typo normalizes the way a browser normalizes it.
    assert_eq!(
        canonicalize_url("https:/example.com").unwrap(),
        "https://example.com/"
    );
}

#[test]
fn a_schemeless_url_may_carry_an_absolute_url_in_its_query_or_fragment() {
    // A Photopea or vscode.dev deep link has exactly this shape.
    assert_eq!(
        canonicalize_url("example.com/?next=https://other.test").unwrap(),
        "https://example.com/?next=https://other.test"
    );
    assert_eq!(
        canonicalize_url("photopea.com/#https://example.com/file.psd").unwrap(),
        "https://photopea.com/#https://example.com/file.psd"
    );
}

#[test]
fn a_host_and_port_is_not_mistaken_for_a_scheme() {
    assert_eq!(
        canonicalize_url("twitter.com:8443").unwrap(),
        "https://twitter.com:8443/"
    );
    assert_eq!(
        canonicalize_url("twitter.com:8443/path").unwrap(),
        "https://twitter.com:8443/path"
    );
}

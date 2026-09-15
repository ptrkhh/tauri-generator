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
            assert_eq!(
                nav_verdict(url, NavEvent::TopLevel),
                Verdict::Allow,
                "{url}"
            );
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
            assert_eq!(
                nav_verdict(url, NavEvent::TopLevel),
                Verdict::Block,
                "{url}"
            );
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
        assert!(!is_trusted_origin(
            &trusted,
            "https://www.photopea.com:8443/"
        ));
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

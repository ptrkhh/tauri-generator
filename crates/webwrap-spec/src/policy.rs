use url::Url;

/// Which kind of navigation is being decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavEvent {
    TopLevel,
    NewWindow,
    /// A navigation inside an iframe.
    Frame,
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
    let parsed = Url::parse(url).ok();
    let scheme = parsed.as_ref().map(Url::scheme);
    match (event, scheme) {
        // A pop-up is handed to the system browser, so only the two schemes a
        // browser is expected to take may take that path. `OpenExternally`
        // becomes a shell open in the runtime, and letting remote page content
        // choose the scheme would let it choose which registered Windows
        // protocol handler launches — the `ms-msdt:` and `search-ms:` class of
        // problem. `http` is safe to hand over because the system browser, not
        // this app, renders it.
        (NavEvent::NewWindow, Some("https" | "http")) => Verdict::OpenExternally,
        (NavEvent::NewWindow, _) => Verdict::Block,
        (NavEvent::TopLevel, Some("https")) => Verdict::Allow,
        (NavEvent::TopLevel, _) => Verdict::Block,
        // Frames may use every scheme a page renders by itself, so sites that
        // embed blob: or data: frames keep working. Anything else would be
        // handed to a Windows protocol handler.
        (NavEvent::Frame, Some("https" | "http" | "blob" | "data")) => Verdict::Allow,
        (NavEvent::Frame, Some("about"))
            if parsed
                .as_ref()
                .is_some_and(|url| matches!(url.path(), "blank" | "srcdoc")) =>
        {
            Verdict::Allow
        }
        (NavEvent::Frame, _) => Verdict::Block,
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
    fn only_browser_schemes_may_open_in_a_new_window() {
        for url in ["https://example.com/", "http://example.com/"] {
            assert_eq!(
                nav_verdict(url, NavEvent::NewWindow),
                Verdict::OpenExternally,
                "{url}"
            );
        }
    }

    #[test]
    fn a_new_window_never_hands_a_non_browser_scheme_to_the_shell() {
        // OpenExternally becomes a shell open. If remote page content could
        // pick the scheme it could pick which Windows protocol handler
        // launches, which is the ms-msdt:/search-ms: class of problem.
        for url in [
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "file:///C:/Windows/System32/calc.exe",
            "ms-msdt:/id PCWDiagnostic",
            "search-ms:query=x",
            "vbscript:msgbox(1)",
            "shell:startup",
            "garbage",
            "",
        ] {
            assert_eq!(
                nav_verdict(url, NavEvent::NewWindow),
                Verdict::Block,
                "{url}"
            );
        }
    }

    #[test]
    fn frames_may_load_web_content_but_never_an_external_scheme() {
        for url in [
            "https://example.com/",
            "http://example.com/",
            "blob:https://example.com/0f8f",
            "data:text/html,hi",
            "about:blank",
            "about:srcdoc",
        ] {
            assert_eq!(nav_verdict(url, NavEvent::Frame), Verdict::Allow, "{url}");
        }
        for url in [
            "search-ms:query=x",
            "ms-msdt:/id PCWDiagnostic",
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "about:settings",
            "mailto:a@b.test",
            "garbage",
        ] {
            assert_eq!(nav_verdict(url, NavEvent::Frame), Verdict::Block, "{url}");
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

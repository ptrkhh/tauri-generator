//! The generic adapter (`adapter: none`): the page script in `generic.js`
//! offers each file through `launchQueue`, the upload box, or a drop, and
//! reports which one took it.

/// The page script for an app that trusts `origins`.
pub fn script(origins: &[String]) -> String {
    let origins = serde_json::to_string(origins).expect("strings always serialize");
    include_str!("generic.js").replace("__WEBWRAP_TRUSTED__", &origins)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_gets_the_origins_as_a_javascript_array() {
        let script = script(&["https://twitter.com".into()]);
        assert!(script.contains(r#"const trusted = ["https://twitter.com"];"#));
        assert!(!script.contains("__WEBWRAP_TRUSTED__"));
    }
}

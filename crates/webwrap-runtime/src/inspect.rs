use std::path::Path;

use serde_json::{json, Value};
use webwrap_spec::{nav_verdict, Adapter, AppSpec, NavEvent, Verdict};

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
    let registration = crate::assoc::Registration::new(
        &spec.app_id,
        &spec.display_name,
        &spec.file_associations,
        crate::assoc::this_exe().unwrap_or_default(),
    );
    let report = json!({
        "schemaVersion": spec.schema_version,
        "appId": spec.app_id,
        "displayName": spec.display_name,
        "slug": spec.slug,
        "launchUrl": spec.launch_url,
        "trustedOrigins": spec.trusted_origins,
        "adapter": spec.adapter,
        // The app's kind, for F's label; which tier takes a file is per file.
        "fileTier": match spec.adapter {
            Adapter::None => "generic",
            Adapter::Photopea | Adapter::VscodeDev => "verified",
        },
        "fileAssociations": spec.file_associations,
        "associations": {
            "progId": registration.prog_id,
            "extensions": registration.extensions,
            "registeredHere": registration.registered_here(),
        },
        "dnsMode": spec.dns_mode,
        "appVersion": spec.app_version,
        "runtimeArch": std::env::consts::ARCH,
        "webview2DataDir": data_dir.to_string_lossy(),
        "navVerdicts": verdicts,
    });
    println!("{report}");
    std::process::exit(0)
}

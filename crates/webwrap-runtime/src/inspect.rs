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

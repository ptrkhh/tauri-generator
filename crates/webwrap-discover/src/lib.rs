//! Website discovery for the generator (sub-project F1): the URL to wrap, a
//! name, icon candidates, and suggested file associations.
//!
//! Every request goes through `fetch`, which carries all the limits: https
//! only, at most `MAX_REDIRECTS` redirects, `TIMEOUT` per request, a size cap,
//! and an allowed set of content types. Images are not decoded here: the
//! generator's page does that, and pack checks the PNG it makes.

use std::collections::HashSet;
use std::io::Read;
use std::time::Duration;

use dom_query::Document;
use reqwest::blocking::Client;
use reqwest::header::{CONTENT_TYPE, LOCATION};
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
use url::Url;
use webwrap_spec::{app_identity, canonicalize_url, check_assoc, Adapter, Assoc};

pub const MAX_REDIRECTS: usize = 5;
pub const TIMEOUT: Duration = Duration::from_secs(10);
pub const MAX_DOCUMENT_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_CANDIDATES: usize = 12;

/// Image types the generator's WebView2 decodes (SVG is skipped by choice).
const IMAGE_TYPES: &[&str] = &[
    "image/png",
    "image/x-icon",
    "image/vnd.microsoft.icon",
    "image/ico",
    "image/jpeg",
    "image/webp",
    "image/gif",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// Where it was found: "manifest", "apple-touch-icon", "icon", "favicon".
    pub label: &'static str,
    pub url: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub assoc: Assoc,
    pub ticked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    /// Canonical URL the app wraps: the typed one, moved along permanent
    /// redirects only.
    pub wrapped_url: String,
    pub app_id: String,
    /// Derived from the host the user typed, so `twitter.com` stays `twitter`.
    pub slug: String,
    pub name: String,
    pub adapter: Adapter,
    pub associations: Vec<Row>,
    pub candidates: Vec<Candidate>,
    /// Why the page could not be read, if it could not. Discovery still
    /// returns the typed URL, the host as the name, and the presets.
    pub problem: Option<String>,
}

/// The client every discovery request uses. `extra_root` is a PEM
/// certificate trusted on top of the platform store; only tests pass one.
pub fn client(extra_root: Option<&[u8]>) -> Result<Client, String> {
    let mut builder = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT)
        .https_only(true)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) WebWrap-Generator");
    if let Some(pem) = extra_root {
        let cert = reqwest::Certificate::from_pem(pem).map_err(|error| error.to_string())?;
        builder = builder.tls_certs_merge([cert]);
    }
    builder.build().map_err(|error| chain(&error))
}

/// An error with its causes: reqwest's own message is only "builder error".
fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text += &format!(": {cause}");
        source = cause.source();
    }
    text
}

struct Fetched {
    /// Where the body came from, after every redirect.
    url: Url,
    /// The start, moved along the unbroken run of permanent redirects.
    permanent: Url,
    content_type: String,
    bytes: Vec<u8>,
}

fn fetch(
    client: &Client,
    start: Url,
    accept: fn(&str) -> bool,
    limit: u64,
) -> Result<Fetched, String> {
    let mut url = start.clone();
    let mut permanent = start;
    let mut unbroken = true;
    for _ in 0..=MAX_REDIRECTS {
        if url.scheme() != "https" {
            return Err(format!("{url} is not https"));
        }
        let response = client
            .get(url.clone())
            .send()
            .map_err(|error| format!("{url} could not be fetched: {}", chain(&error)))?;
        let status = response.status();
        if status.is_redirection() {
            let next = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|location| url.join(location).ok())
                .ok_or_else(|| format!("{url} redirected without a usable Location"))?;
            unbroken &= matches!(
                status,
                StatusCode::MOVED_PERMANENTLY | StatusCode::PERMANENT_REDIRECT
            );
            if unbroken {
                permanent = next.clone();
            }
            url = next;
            continue;
        }
        if !status.is_success() {
            return Err(format!("{url} answered {status}"));
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if !accept(&content_type) {
            return Err(format!(
                "{url} is {content_type:?}, which is not expected here"
            ));
        }
        let mut bytes = Vec::new();
        response
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("{url} could not be read: {error}"))?;
        if bytes.len() as u64 > limit {
            return Err(format!("{url} is larger than {limit} bytes"));
        }
        return Ok(Fetched {
            url,
            permanent,
            content_type,
            bytes,
        });
    }
    Err(format!("more than {MAX_REDIRECTS} redirects"))
}

fn is_page(content_type: &str) -> bool {
    matches!(content_type, "text/html" | "application/xhtml+xml")
}

fn is_json(content_type: &str) -> bool {
    content_type.ends_with("json")
}

fn is_image(content_type: &str) -> bool {
    IMAGE_TYPES.contains(&content_type)
}

/// The adapter a wrapped host gets (D: adapters exist only for these).
pub fn adapter_for(host: &str) -> Adapter {
    match host {
        "www.photopea.com" | "photopea.com" => Adapter::Photopea,
        "vscode.dev" => Adapter::VscodeDev,
        _ => Adapter::None,
    }
}

pub fn discover(client: &Client, typed: &str) -> Result<Discovery, String> {
    let canonical = canonicalize_url(typed).map_err(|error| error.to_string())?;
    let typed_url = Url::parse(&canonical).expect("a canonical URL parses");
    let host = typed_url.host_str().unwrap_or_default().to_owned();
    let slug = webwrap_pack::derive_slug(&canonical);

    let mut name = None;
    let mut icons = Vec::new();
    let mut declared = Vec::new();
    let (wrapped_url, problem) = match fetch(client, typed_url, is_page, MAX_DOCUMENT_BYTES) {
        Ok(page) => {
            let html = String::from_utf8_lossy(&page.bytes);
            let doc = Document::from(html.as_ref());
            let hint = Url::parse(&canonicalize_url(page.permanent.as_str()).unwrap_or_default())
                .ok()
                .and_then(|wrapped| manifest_hint(adapter_for(wrapped.host_str()?), &wrapped));
            let manifest = manifest_link(&doc, &page.url).or(hint).and_then(|link| {
                let fetched = fetch(client, link, is_json, MAX_DOCUMENT_BYTES).ok()?;
                let value: Value = serde_json::from_slice(&fetched.bytes).ok()?;
                Some((fetched.url, value))
            });
            if let Some((manifest_url, manifest)) = &manifest {
                name = manifest_name(manifest);
                icons.extend(manifest_icons(manifest, manifest_url));
                declared = file_handlers(manifest);
            }
            name = name.or_else(|| page_name(&doc));
            icons.extend(link_icons(&doc, &page.url));
            let wrapped =
                canonicalize_url(page.permanent.as_str()).map_err(|error| error.to_string())?;
            (wrapped, None)
        }
        Err(error) => (canonical, Some(error)),
    };
    let wrapped_host = Url::parse(&wrapped_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default();
    if let Ok(favicon) = Url::parse(&wrapped_url).and_then(|url| url.join("/favicon.ico")) {
        icons.push(Icon {
            label: "favicon",
            url: favicon,
            size: None,
            maskable: false,
        });
    }
    let adapter = adapter_for(&wrapped_host);

    let mut seen = HashSet::new();
    icons.retain(|icon| seen.insert(icon.url.clone()));
    // shortcut: fetched one at a time, so 12 slow servers take 12 timeouts;
    // fetch them on threads if that is ever seen.
    let candidates = icons
        .into_iter()
        .take(MAX_CANDIDATES)
        .filter_map(|icon| {
            let fetched = fetch(client, icon.url, is_image, MAX_IMAGE_BYTES).ok()?;
            Some(Candidate {
                label: icon.label,
                url: fetched.url.to_string(),
                content_type: fetched.content_type,
                bytes: fetched.bytes,
            })
        })
        .collect();

    Ok(Discovery {
        app_id: app_identity(&wrapped_url),
        wrapped_url,
        slug,
        name: name.unwrap_or(host),
        adapter,
        associations: with_presets(declared, adapter),
        candidates,
        problem,
    })
}

/// `rel` holds a space-separated, case-insensitive token list.
fn has_rel(rel: &str, token: &str) -> bool {
    rel.split_ascii_whitespace()
        .any(|t| t.eq_ignore_ascii_case(token))
}

/// Where an adapter site keeps its manifest when its page only links it
/// from a script: vscode.dev adds its `<link rel="manifest">` at run time.
fn manifest_hint(adapter: Adapter, wrapped: &Url) -> Option<Url> {
    match adapter {
        Adapter::VscodeDev => wrapped.join("/static/stable/code.webmanifest").ok(),
        Adapter::Photopea | Adapter::None => None,
    }
}

fn manifest_link(doc: &Document, page: &Url) -> Option<Url> {
    doc.select("link[rel][href]").iter().find_map(|link| {
        let rel = link.attr("rel")?;
        has_rel(&rel, "manifest").then(|| page.join(&link.attr("href")?).ok())?
    })
}

fn text(value: &Value) -> Option<String> {
    let text = value.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn manifest_name(manifest: &Value) -> Option<String> {
    text(&manifest["short_name"]).or_else(|| text(&manifest["name"]))
}

fn page_name(doc: &Document) -> Option<String> {
    let meta = doc
        .select(r#"meta[name="application-name"]"#)
        .iter()
        .find_map(|meta| meta.attr("content").map(|c| c.trim().to_owned()))
        .filter(|name| !name.is_empty());
    meta.or_else(|| {
        let title = doc.select("title").text();
        let cut = [" | ", " - ", " – "]
            .iter()
            .filter_map(|separator| title.find(separator))
            .min()
            .unwrap_or(title.len());
        let name = title[..cut].trim();
        (!name.is_empty()).then(|| name.to_owned())
    })
}

#[derive(Debug)]
struct Icon {
    label: &'static str,
    url: Url,
    /// The largest declared square size, if any.
    size: Option<u32>,
    /// A manifest icon meant only for cropping into a shape: padded as a
    /// desktop icon, so it goes after the others.
    maskable: bool,
}

fn largest_square(sizes: &str) -> Option<u32> {
    sizes
        .split_ascii_whitespace()
        .filter_map(|size| {
            let (w, h) = size
                .to_ascii_lowercase()
                .split_once('x')
                .map(|(w, h)| (w.to_owned(), h.to_owned()))?;
            let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
            (w == h).then_some(w)
        })
        .max()
}

fn is_svg(kind: &str, url: &Url) -> bool {
    kind.to_ascii_lowercase().contains("svg") || url.path().to_ascii_lowercase().ends_with(".svg")
}

/// Maskable-only icons last; then declared square sizes first, largest
/// first; the rest keep their order.
fn by_size(mut icons: Vec<Icon>) -> Vec<Icon> {
    icons.sort_by_key(|icon| {
        (
            icon.maskable,
            std::cmp::Reverse(icon.size.map_or(0, |size| size + 1)),
        )
    });
    icons
}

fn manifest_icons(manifest: &Value, manifest_url: &Url) -> Vec<Icon> {
    let Some(list) = manifest["icons"].as_array() else {
        return Vec::new();
    };
    by_size(
        list.iter()
            .filter_map(|icon| {
                let url = manifest_url.join(icon["src"].as_str()?).ok()?;
                let kind = icon["type"].as_str().unwrap_or_default();
                let purpose = icon["purpose"].as_str().unwrap_or("any");
                (!is_svg(kind, &url)).then(|| Icon {
                    label: "manifest",
                    size: largest_square(icon["sizes"].as_str().unwrap_or_default()),
                    maskable: !purpose.split_ascii_whitespace().any(|p| p == "any"),
                    url,
                })
            })
            .collect(),
    )
}

fn link_icons(doc: &Document, page: &Url) -> Vec<Icon> {
    let links: Vec<_> = doc
        .select("link[rel][href]")
        .iter()
        .filter_map(|link| {
            let rel = link.attr("rel")?.to_string();
            let url = page.join(&link.attr("href")?).ok()?;
            let kind = link.attr("type").map(|t| t.to_string()).unwrap_or_default();
            let size = largest_square(
                &link
                    .attr("sizes")
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
            );
            (!is_svg(&kind, &url)).then_some((rel, url, size))
        })
        .collect();
    let pick = |label: &'static str, tokens: &[&str]| {
        by_size(
            links
                .iter()
                .filter(|(rel, ..)| tokens.iter().any(|token| has_rel(rel, token)))
                .map(|(_, url, size)| Icon {
                    label,
                    url: url.clone(),
                    size: *size,
                    maskable: false,
                })
                .collect(),
        )
    };
    let mut icons = pick(
        "apple-touch-icon",
        &["apple-touch-icon", "apple-touch-icon-precomposed"],
    );
    // "shortcut icon" holds the token "icon" too.
    icons.extend(pick("icon", &["icon"]));
    icons
}

/// A manifest's `file_handlers`, one row per extension, lowercased, keeping
/// only what the schema accepts. The first claim of an extension wins.
fn file_handlers(manifest: &Value) -> Vec<Assoc> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for handler in manifest["file_handlers"].as_array().into_iter().flatten() {
        let Some(accept) = handler["accept"].as_object() else {
            continue;
        };
        for (mime, extensions) in accept {
            let extensions: Vec<&str> = match extensions {
                Value::String(one) => vec![one.as_str()],
                Value::Array(many) => many.iter().filter_map(Value::as_str).collect(),
                _ => continue,
            };
            for extension in extensions {
                let extension = extension.trim().to_ascii_lowercase();
                let description = text(&handler["name"]).unwrap_or_else(|| {
                    format!(
                        "{} file",
                        extension.trim_start_matches('.').to_ascii_uppercase()
                    )
                });
                let assoc = Assoc {
                    extension,
                    mime_type: mime.clone(),
                    description,
                };
                if check_assoc(&assoc).is_ok() && seen.insert(assoc.extension.clone()) {
                    found.push(assoc);
                }
            }
        }
    }
    found
}

fn preset(extension: &str, mime_type: &str, description: &str) -> Assoc {
    Assoc {
        extension: extension.into(),
        mime_type: mime_type.into(),
        description: description.into(),
    }
}

/// The adapter's tested presets, merged over the manifest's rows by
/// extension (the schema refuses an extension claimed twice). Manifest rows
/// are ticked only for a site with no adapter.
pub fn with_presets(declared: Vec<Assoc>, adapter: Adapter) -> Vec<Row> {
    let presets: Vec<Row> = match adapter {
        Adapter::Photopea => vec![Row {
            assoc: preset(".psd", "image/vnd.adobe.photoshop", "Photoshop document"),
            ticked: true,
        }],
        Adapter::VscodeDev => {
            let mut rows = vec![Row {
                assoc: preset(".md", "text/markdown", "Markdown document"),
                ticked: true,
            }];
            for (extension, mime_type, description) in [
                (".txt", "text/plain", "Text file"),
                (".json", "application/json", "JSON file"),
                (".js", "text/javascript", "JavaScript file"),
                (".ts", "text/x-typescript", "TypeScript file"),
                (".css", "text/css", "CSS file"),
                (".html", "text/html", "HTML file"),
                (".py", "text/x-python", "Python file"),
                (".yaml", "application/yaml", "YAML file"),
                (".xml", "application/xml", "XML file"),
            ] {
                rows.push(Row {
                    assoc: preset(extension, mime_type, description),
                    ticked: false,
                });
            }
            rows
        }
        Adapter::None => Vec::new(),
    };
    let ticked = adapter == Adapter::None;
    let mut rows: Vec<Row> = declared
        .into_iter()
        .filter(|assoc| {
            !presets
                .iter()
                .any(|row| row.assoc.extension == assoc.extension)
        })
        .map(|assoc| Row { assoc, ticked })
        .collect();
    let mut all = presets;
    all.append(&mut rows);
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_replace_manifest_rows_and_adapter_sites_get_them_unticked() {
        let declared = vec![
            preset(".psd", "image/x-photoshop", "From the manifest"),
            preset(".png", "image/png", "PNG"),
        ];
        let rows = with_presets(declared.clone(), Adapter::Photopea);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].assoc.description, "Photoshop document");
        assert!(rows[0].ticked);
        assert_eq!(
            (rows[1].assoc.extension.as_str(), rows[1].ticked),
            (".png", false)
        );

        let rows = with_presets(declared, Adapter::None);
        assert!(rows.iter().all(|row| row.ticked));
        assert_eq!(rows[0].assoc.description, "From the manifest");
    }

    #[test]
    fn every_preset_passes_the_schema() {
        for adapter in [Adapter::Photopea, Adapter::VscodeDev] {
            for row in with_presets(Vec::new(), adapter) {
                check_assoc(&row.assoc).unwrap();
            }
        }
        let vscode = with_presets(Vec::new(), Adapter::VscodeDev);
        let ticked: Vec<_> = vscode
            .iter()
            .filter(|row| row.ticked)
            .map(|row| row.assoc.extension.as_str())
            .collect();
        assert_eq!(ticked, [".md"]);
    }

    #[test]
    fn adapters_come_from_the_wrapped_host() {
        assert_eq!(adapter_for("www.photopea.com"), Adapter::Photopea);
        assert_eq!(adapter_for("photopea.com"), Adapter::Photopea);
        assert_eq!(adapter_for("vscode.dev"), Adapter::VscodeDev);
        assert_eq!(adapter_for("x.com"), Adapter::None);
    }

    #[test]
    fn sizes_pick_the_largest_square() {
        assert_eq!(largest_square("16x16 32x32 48X48"), Some(48));
        assert_eq!(largest_square("192x96 any"), None);
        assert_eq!(largest_square(""), None);
    }
}

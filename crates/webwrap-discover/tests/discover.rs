//! Discovery against a local HTTPS server, through the real `client` with the
//! fixture CA added, so every limit is the one the generator uses.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use webwrap_discover::{client, discover, Discovery, MAX_DOCUMENT_BYTES, MAX_IMAGE_BYTES};

const CA: &[u8] = include_bytes!("fixtures/ca.pem");
const CERT: &[u8] = include_bytes!("fixtures/localhost.pem");
const KEY: &[u8] = include_bytes!("fixtures/localhost.key");
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake";

#[derive(Clone)]
struct Reply {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

fn ok(content_type: &str, body: impl Into<Vec<u8>>) -> Reply {
    Reply {
        status: 200,
        headers: vec![("Content-Type", content_type.into())],
        body: body.into(),
    }
}

fn redirect(status: u16, location: &str) -> Reply {
    Reply {
        status,
        headers: vec![("Location", location.into())],
        body: Vec::new(),
    }
}

/// Serve `routes` (path to reply) on a random localhost port; unknown paths
/// get 404. Returns the port. The server lives until the test process ends.
fn serve(routes: Vec<(&str, Reply)>) -> u16 {
    let routes: HashMap<String, Reply> = routes
        .into_iter()
        .map(|(path, reply)| (path.to_owned(), reply))
        .collect();
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            CertificateDer::pem_slice_iter(CERT)
                .collect::<Result<_, _>>()
                .unwrap(),
            PrivateKeyDer::from_pem_slice(KEY).unwrap(),
        )
        .unwrap();
    let config = Arc::new(config);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let config = config.clone();
            let routes = routes.clone();
            std::thread::spawn(move || {
                let connection = rustls::ServerConnection::new(config).unwrap();
                let mut tls = rustls::StreamOwned::new(connection, stream);
                let mut reader = BufReader::new(&mut tls);
                let mut request = String::new();
                if reader.read_line(&mut request).is_err() {
                    return;
                }
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let reply = routes.get(&path).cloned().unwrap_or(Reply {
                    status: 404,
                    headers: Vec::new(),
                    body: Vec::new(),
                });
                let mut head = format!(
                    "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    reply.status,
                    reply.body.len()
                );
                for (name, value) in &reply.headers {
                    head += &format!("{name}: {value}\r\n");
                }
                head += "\r\n";
                let _ = tls.write_all(head.as_bytes());
                let _ = tls.write_all(&reply.body);
                let _ = tls.flush();
                tls.conn.send_close_notify();
                let _ = tls.flush();
            });
        }
    });
    port
}

fn run(routes: Vec<(&str, Reply)>) -> (u16, Discovery) {
    let port = serve(routes);
    let client = client(Some(CA)).unwrap();
    (
        port,
        discover(&client, &format!("localhost:{port}")).unwrap(),
    )
}

fn labels(found: &Discovery) -> Vec<(&str, String)> {
    found
        .candidates
        .iter()
        .map(|c| (c.label, c.url.rsplit('/').next().unwrap().to_owned()))
        .collect()
}

#[test]
fn a_full_page_gives_manifest_name_ordered_icons_and_declared_associations() {
    let page = r#"<html><head><title>Long Title | Site</title>
        <link rel="manifest" href="/app/manifest.json">
        <link rel="apple-touch-icon" href="/touch.png" sizes="180x180">
        <link rel="icon" type="image/svg+xml" href="/icon.svg">
        <link rel="Shortcut Icon" href="/small.png" sizes="16x16">
        <link rel="icon" href="/big.png" sizes="64x64">
        </head></html>"#;
    let manifest = r#"{"short_name":"Short","name":"Long name",
        "icons":[{"src":"m512.png","sizes":"512x512","purpose":"maskable"},{"src":"i192.png","sizes":"192x192"},{"src":"i512.png","sizes":"512x512","purpose":"any maskable"},
                 {"src":"vector.svg","type":"image/svg+xml","sizes":"any"},{"src":"/touch.png"}],
        "file_handlers":[{"name":"Images","accept":{"image/x-test":[".PSD",".bad!",".psd"],"text/plain":".TXT"}}]}"#;
    let mut routes = vec![
        ("/", ok("text/html; charset=utf-8", page)),
        (
            "/app/manifest.json",
            ok("application/manifest+json", manifest),
        ),
        ("/favicon.ico", ok("image/x-icon", PNG)),
    ];
    for path in [
        "/app/m512.png",
        "/app/i192.png",
        "/app/i512.png",
        "/touch.png",
        "/small.png",
        "/big.png",
    ] {
        routes.push((path, ok("image/png", PNG)));
    }
    let (port, found) = run(routes);

    assert_eq!(found.problem, None);
    assert_eq!(found.wrapped_url, format!("https://localhost:{port}/"));
    assert_eq!(found.name, "Short");
    assert_eq!(found.slug, "localhost");
    // Manifest icons resolve against the manifest; links against the page;
    // largest declared square first within each source, maskable-only
    // last; SVG skipped; a URL
    // seen twice (touch.png) kept at its first place.
    assert_eq!(
        labels(&found),
        [
            ("manifest", "i512.png".into()),
            ("manifest", "i192.png".into()),
            ("manifest", "touch.png".into()),
            ("manifest", "m512.png".into()),
            ("icon", "big.png".into()),
            ("icon", "small.png".into()),
            ("favicon", "favicon.ico".into()),
        ]
    );
    assert_eq!(found.candidates[0].bytes, PNG);
    let rows: Vec<_> = found
        .associations
        .iter()
        .map(|row| {
            (
                row.assoc.extension.as_str(),
                row.assoc.description.as_str(),
                row.ticked,
            )
        })
        .collect();
    assert_eq!(rows, [(".psd", "Images", true), (".txt", "Images", true)]);
}

#[test]
fn the_name_falls_back_to_application_name_then_the_title_then_the_host() {
    let (_, found) = run(vec![(
        "/",
        ok(
            "text/html",
            r#"<meta name="application-name" content=" App Name "><title>T</title>"#,
        ),
    )]);
    assert_eq!(found.name, "App Name");

    let (_, found) = run(vec![(
        "/",
        ok("text/html", "<title>Inbox - Mail – Company | Org</title>"),
    )]);
    assert_eq!(found.name, "Inbox");

    let (_, found) = run(vec![("/", ok("text/html", "<p>nothing</p>"))]);
    assert_eq!(found.name, "localhost");
}

#[test]
fn only_permanent_redirects_from_the_start_move_the_wrapped_url() {
    let (port, found) = run(vec![
        ("/", redirect(301, "/moved")),
        ("/moved", redirect(308, "/home")),
        ("/home", redirect(302, "/login?next=home")),
        ("/login?next=home", redirect(301, "/signin")),
        ("/signin", ok("text/html", "<title>Sign in</title>")),
    ]);
    assert_eq!(found.problem, None);
    assert_eq!(found.wrapped_url, format!("https://localhost:{port}/home"));
    // Metadata comes from the page at the end.
    assert_eq!(found.name, "Sign in");
}

#[test]
fn a_page_that_cannot_be_read_still_gives_a_usable_result() {
    let too_big = vec![b'a'; MAX_DOCUMENT_BYTES as usize + 1];
    for (reply, why) in [
        (redirect(301, "http://localhost/"), "is not https"),
        (ok("text/plain", "<title>x</title>"), "not expected here"),
        (ok("text/html", too_big), "larger than"),
        (
            Reply {
                status: 500,
                headers: Vec::new(),
                body: Vec::new(),
            },
            "500",
        ),
    ] {
        let (port, found) = run(vec![("/", reply)]);
        let problem = found.problem.unwrap_or_default();
        assert!(problem.contains(why), "{why}: {problem}");
        assert_eq!(found.wrapped_url, format!("https://localhost:{port}/"));
        assert_eq!(found.name, "localhost");
    }

    let mut loop_routes = Vec::new();
    let paths = ["/", "/1", "/2", "/3", "/4", "/5", "/6"];
    for pair in paths.windows(2) {
        loop_routes.push((pair[0], redirect(301, pair[1])));
    }
    let (_, found) = run(loop_routes);
    assert!(found
        .problem
        .unwrap_or_default()
        .contains("more than 5 redirects"));
}

#[test]
fn icons_of_the_wrong_type_or_too_large_are_dropped() {
    let page = r#"<link rel="icon" href="/octet.png"><link rel="icon" href="/huge.png"><link rel="icon" href="/fine.png">"#;
    let (_, found) = run(vec![
        ("/", ok("text/html", page)),
        ("/octet.png", ok("application/octet-stream", PNG)),
        (
            "/huge.png",
            ok("image/png", vec![0u8; MAX_IMAGE_BYTES as usize + 1]),
        ),
        ("/fine.png", ok("image/png", PNG)),
    ]);
    assert_eq!(labels(&found), [("icon", "fine.png".into())]);
}

#[test]
fn a_non_https_url_is_refused_outright() {
    let client = client(Some(CA)).unwrap();
    assert!(discover(&client, "http://localhost/").is_err());
}

/// Live sites, run by hand: `cargo test -p webwrap-discover -- --ignored --nocapture`.
#[test]
#[ignore]
fn live_sites_wrap_the_permanent_target_and_find_icons() {
    let client = client(None).unwrap();
    for (typed, wrapped, slug, adapter) in [
        (
            "twitter.com",
            "https://x.com/",
            "twitter",
            webwrap_spec::Adapter::None,
        ),
        (
            "photopea.com",
            "https://www.photopea.com/",
            "photopea",
            webwrap_spec::Adapter::Photopea,
        ),
        (
            "vscode.dev",
            "https://vscode.dev/",
            "vscode",
            webwrap_spec::Adapter::VscodeDev,
        ),
    ] {
        let found = discover(&client, typed).unwrap();
        let ticked: Vec<_> = found
            .associations
            .iter()
            .filter(|row| row.ticked)
            .map(|row| row.assoc.extension.clone())
            .collect();
        println!(
            "{typed}: {} name={:?} problem={:?} ticked={ticked:?} rows={} candidates={:?}",
            found.wrapped_url,
            found.name,
            found.problem,
            found.associations.len(),
            found
                .candidates
                .iter()
                .map(|c| format!("{} {} {}B", c.label, c.content_type, c.bytes.len()))
                .collect::<Vec<_>>()
        );
        assert_eq!(found.problem, None, "{typed}");
        assert_eq!(
            (
                found.wrapped_url.as_str(),
                found.slug.as_str(),
                found.adapter
            ),
            (wrapped, slug, adapter)
        );
        assert!(!found.candidates.is_empty(), "{typed}: no icons");
    }
}

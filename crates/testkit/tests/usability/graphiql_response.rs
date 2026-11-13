//! Verifies fixed assets, configuration escaping, and path constraints through the neutral public API.

use chigiri::{Endpoint, ResponseParts, graphiql::Paths};

use super::support::resolvers;

fn header<'a>(response: &'a ResponseParts, name: &str) -> &'a str {
    &response
        .headers
        .iter()
        .find(|item| item.name == name)
        .unwrap()
        .value
}

#[test]
fn dx35_14_16_23_neutral_html_assets_and_head_are_fixed_and_safe() {
    let endpoint = Endpoint::builder(resolvers())
        .without_context()
        .graphiql(true)
        .build()
        .unwrap();
    let view = endpoint.graphiql().unwrap();
    let inventory: serde_json::Value =
        serde_json::from_str(include_str!("../../../graphiql/asset-inventory.json")).unwrap();
    for paths in [
        Paths::default(),
        Paths::new("/query-v2", "/console/")
            .unwrap()
            .with_prefix("/api/v1")
            .unwrap(),
    ] {
        let html = view.respond(&paths, "GET", paths.ui());
        assert_eq!(html.status, 200);
        let text = std::str::from_utf8(&html.body).unwrap();
        assert!(text.contains(&format!("data-endpoint=\"{}\"", paths.graphql())));
        assert!(text.contains(&format!("src=\"{}app.js\"", paths.assets_prefix())));
        assert!(text.contains(&format!("href=\"{}app.css\"", paths.assets_prefix())));
        assert!(!text.contains("http:"));
        assert!(!text.contains("https:"));
        assert_eq!(text.matches("<script").count(), 1);
        assert!(!text.contains("localStorage"));
        let mut files = vec![(paths.ui().to_owned(), "text/html; charset=utf-8")];
        for item in inventory["files"].as_array().unwrap() {
            let name = item["path"].as_str().unwrap();
            let path = format!("{}{name}", paths.assets_prefix());
            let response = view.respond(&paths, "GET", &path);
            assert_eq!(response.body.len() as u64, item["bytes"].as_u64().unwrap());
            let expected = std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../graphiql/assets")
                    .join(name),
            )
            .unwrap();
            assert_eq!(response.body, expected);
            files.push((path, item["content_type"].as_str().unwrap()));
        }
        for (path, content_type) in files {
            let get = view.respond(&paths, "GET", &path);
            let head = view.respond(&paths, "HEAD", &path);
            assert_eq!(get.status, 200);
            assert_eq!(head.status, 200);
            assert_eq!(head.headers, get.headers);
            assert!(head.body.is_empty());
            assert_eq!(header(&get, "content-length"), get.body.len().to_string());
            assert_eq!(header(&get, "content-type"), content_type);
            assert_eq!(header(&get, "x-content-type-options"), "nosniff");
            assert_eq!(header(&get, "cache-control"), "no-store");
            assert_eq!(header(&get, "referrer-policy"), "no-referrer");
            let csp = header(&get, "content-security-policy");
            for directive in [
                "script-src 'self'",
                "connect-src 'self'",
                "worker-src 'self'",
                "base-uri 'none'",
                "frame-ancestors 'none'",
            ] {
                assert!(csp.split("; ").any(|item| item == directive), "{csp}");
            }
            assert!(!csp.contains("unsafe-eval"));
            assert!(!csp.contains("blob:"));
            for method in ["POST", "OPTIONS", "PUT", "DELETE", "get"] {
                let response = view.respond(&paths, method, &path);
                assert_eq!(response.status, 405);
                assert_eq!(header(&response, "allow"), "GET, HEAD");
                assert!(response.body.is_empty());
            }
        }
        for tail in [
            "missing.js",
            "../app.js",
            "%61pp.js",
            "app.js/",
            "app.js?next=http://evil",
            "",
            "../../frontend/entry.mjs",
        ] {
            for method in ["GET", "HEAD", "POST"] {
                let response =
                    view.respond(&paths, method, &format!("{}{tail}", paths.assets_prefix()));
                assert_eq!(response.status, 404);
                assert!(response.body.is_empty());
            }
        }
        assert_eq!(view.respond(&paths, "GET", paths.graphql()).status, 404);
    }
}

#[test]
fn dx35_16_static_paths_reject_external_dynamic_and_colliding_routes() {
    let defaults = Paths::default();
    assert_eq!(defaults.graphql(), "/graphql");
    assert_eq!(defaults.ui(), "/graphiql");
    assert_eq!(defaults.assets_prefix(), "/graphiql/assets/");
    assert_eq!(
        defaults.with_prefix("/api").unwrap().graphql(),
        "/api/graphql"
    );
    assert!(Paths::new("/graphql", "/").is_ok());
    for invalid in [
        "",
        "relative",
        "//evil/path",
        "http://evil",
        "https://evil/path",
        "/?query=x",
        "/#fragment",
        "/{id}",
        "/:id",
        "/*rest",
        "/a//b",
        "/a/../b",
        "/./b",
        "/%2e",
        "/a\\b",
        "/résumé",
        "/\n",
        "/\0",
        "/a'",
        "/a\"",
    ] {
        assert!(Paths::new(invalid, "/ui").is_err(), "{invalid:?}");
        let error = Paths::new("/query", invalid).unwrap_err();
        assert_eq!(
            error.to_string(),
            defaults.with_prefix(invalid).unwrap_err().to_string()
        );
    }
    for graphql in ["/ui", "/ui/assets", "/ui/assets/", "/ui/assets/app.js"] {
        assert!(Paths::new(graphql, "/ui").is_err());
    }
    assert!(defaults.with_prefix("/").is_err());
    assert!(defaults.with_prefix("/api/").is_err());
    let boundary = format!("/{}", "a".repeat(4095));
    assert!(Paths::new(&boundary, "/ui").is_ok());
    assert!(Paths::new(&format!("{boundary}a"), "/ui").is_err());
    assert!(Paths::new("/q", &boundary).is_err());
    assert!(
        Paths::new(&boundary, "/ui")
            .unwrap()
            .with_prefix("/api")
            .is_err()
    );
}

//! Exercises HTML and GraphQL over real HTTP on both bridges while preserving external authentication and protocol checks.

use chigiri::{Endpoint, graphiql::Paths};

use super::{
    graphiql_wire::{Bridge, Running},
    support::{execute, resolvers},
};

const INTROSPECTION: &str = "{ __schema { queryType { name } } }";
const AUTH: &str = "Authorization: Bearer test-only\r\n";

#[tokio::test]
async fn dx35_14_16_17_both_helpers_route_default_prefix_and_disabled_without_body_reads() {
    for bridge in [Bridge::Axum, Bridge::Hyper] {
        for prefix in [false, true] {
            let paths = if prefix {
                Paths::new("/query", "/console")
                    .unwrap()
                    .with_prefix("/api/v1")
                    .unwrap()
            } else {
                Paths::default()
            };
            for enabled in [false, true] {
                let endpoint = Endpoint::builder(resolvers())
                    .without_context()
                    .graphiql(enabled)
                    .build()
                    .unwrap();
                let server = Running::spawn(endpoint.clone(), paths.clone(), bridge, false).await;
                let html = server.request("GET", paths.ui(), "", "").await;
                assert_eq!(html.status, if enabled { 200 } else { 404 });
                let mut inventory = vec![paths.ui().to_owned()];
                let assets: serde_json::Value =
                    serde_json::from_str(include_str!("../../../graphiql/asset-inventory.json"))
                        .unwrap();
                for item in assets["files"].as_array().unwrap() {
                    inventory.push(format!(
                        "{}{}",
                        paths.assets_prefix(),
                        item["path"].as_str().unwrap()
                    ));
                }
                for path in inventory {
                    // Send headers only; the UI's body read must expire at the socket timeout.
                    for method in ["GET", "HEAD", "POST", "OPTIONS"] {
                        let response = server
                            .request(method, &path, "Content-Length: 999999\r\n", "")
                            .await;
                        let expected = if enabled {
                            if matches!(method, "GET" | "HEAD") {
                                200
                            } else {
                                405
                            }
                        } else {
                            404
                        };
                        assert_eq!(response.status, expected, "{bridge:?} {path} {method}");
                        if enabled {
                            let neutral =
                                endpoint.graphiql().unwrap().respond(&paths, method, &path);
                            assert_eq!(response.body, neutral.body, "{bridge:?} {path} {method}");
                            for header in neutral.headers {
                                assert_eq!(response.headers.get(&header.name), Some(&header.value));
                            }
                        }
                    }
                }
                for path in [
                    format!("{}missing.js", paths.assets_prefix()),
                    format!("{}../app.js", paths.assets_prefix()),
                    "/unknown".into(),
                ] {
                    for method in ["GET", "HEAD", "POST"] {
                        assert_eq!(
                            server
                                .request(method, &path, "Content-Length: 999999\r\n", "")
                                .await
                                .status,
                            404
                        );
                    }
                }
                if enabled {
                    let extra = server
                        .request(
                            "GET",
                            &format!("{}?endpoint=https://evil.invalid&token=secret", paths.ui()),
                            "",
                            "",
                        )
                        .await;
                    assert_eq!(extra.body, html.body);
                    assert!(
                        !String::from_utf8(html.body)
                            .unwrap()
                            .contains("forged.invalid")
                    );
                }
                let response = server.graphql(paths.graphql(), "{ greeting }", "").await;
                assert_eq!(response.status, 200);
                assert_eq!(response.json()["data"]["greeting"], "hello");
                let response = server.graphql(paths.graphql(), INTROSPECTION, "").await;
                assert_eq!(response.json().get("errors").is_none(), enabled);
                assert_eq!(execute(&endpoint, INTROSPECTION).errors.is_empty(), enabled);
                let html_accept = server
                    .request(
                        "GET",
                        &format!("{}?query=%7Bgreeting%7D", paths.graphql()),
                        "Accept: text/html\r\nX-Chigiri-Preflight: 1\r\n",
                        "",
                    )
                    .await;
                assert_eq!(html_accept.status, 406);
                assert!(!String::from_utf8_lossy(&html_accept.body).contains("<!doctype html>"));
                assert_eq!(endpoint.in_flight_requests(), 0);
                server.stop().await;
            }
        }
    }
}

#[tokio::test]
async fn dx35_17_18_outer_auth_protects_all_routes_and_graphql_keeps_csrf_and_method_checks() {
    for bridge in [Bridge::Axum, Bridge::Hyper] {
        let endpoint = Endpoint::builder(resolvers())
            .without_context()
            .graphiql(true)
            .build()
            .unwrap();
        let server = Running::spawn(endpoint, Paths::default(), bridge, true).await;
        for path in ["/graphql", "/graphiql", "/graphiql/assets/app.js"] {
            assert_eq!(
                server
                    .request("GET", path, "Content-Length: 999999\r\n", "")
                    .await
                    .status,
                401
            );
        }
        for path in ["/graphiql", "/graphiql/assets/app.js"] {
            assert_eq!(server.request("GET", path, AUTH, "").await.status, 200);
        }
        let query = server.graphql("/graphql", "{ greeting }", AUTH).await;
        assert_eq!(query.status, 200);
        assert_eq!(query.json()["data"]["greeting"], "hello");
        assert_eq!(
            server
                .graphql("/graphql", "mutation { rename(name: \"new\") }", AUTH)
                .await
                .json()["data"]["rename"],
            "new"
        );
        assert!(
            server
                .graphql("/graphql", INTROSPECTION, AUTH)
                .await
                .json()
                .get("errors")
                .is_none()
        );
        assert_eq!(
            server
                .request(
                    "GET",
                    "/graphql?query=%7Bgreeting%7D",
                    &format!("{AUTH}Accept: */*\r\n"),
                    ""
                )
                .await
                .status,
            403
        );
        assert_eq!(server.request("GET", "/graphql?query=mutation%7Brename(name:%22new%22)%7D", &format!("{AUTH}Accept: application/graphql-response+json\r\nX-Chigiri-Preflight: 1\r\n"), "").await.status, 405);
        // General helper does not add CORS policy. The Origin license is owned by an outside of application.
        let origin = server
            .graphql(
                "/graphql",
                "{ greeting }",
                &format!("{AUTH}Origin: https://unknown.invalid\r\n"),
            )
            .await;
        assert_eq!(origin.status, 200);
        assert!(!origin.headers.contains_key("access-control-allow-origin"));
        server.stop().await;
    }
}

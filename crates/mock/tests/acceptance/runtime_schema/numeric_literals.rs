//! Verify over real HTTP that INTEGER literals, JSON variables, and defaults preserve exact values.

use super::{data, failure, init, ready};
use crate::support::{Directory, Server};
use serde_json::json;

#[test]
fn rsm_04_09_custom_integer_decimal_and_exponent_literals_do_not_round() {
    let _test_case = crate::support::test_case();
    let (_, config) = init(
        r#"
        scalar Count @representation(type: INTEGER)
        scalar Data @representation(type: JSON)
        type Query {
            count(value:Count! = 9007199254740994.0):Count!
            echo(value:Data!):Data!
        }
    "#,
    );
    let mut server = Server::start(&ready(
        &config,
        &[
            ("Query.count", "return args.value"),
            ("Query.echo", "return args.value"),
        ],
    ));
    for raw in ["9007199254740994.0", "9007199254740994e0"] {
        assert_eq!(
            data(&server, &format!("{{count(value:{raw})}}")),
            json!({"count":9007199254740994_i64})
        );
    }
    // Fractional decimals retain the established finite f64 conversion for literals and variables.
    for raw in ["1.0000000000000001", "0.99999999999999999"] {
        assert_eq!(
            data(&server, &format!("{{count(value:{raw})}}")),
            json!({"count":1})
        );
        let response = server.http(
            "POST",
            &[("Content-Type", "application/json")],
            &format!(
                r#"{{"query":"query($v:Count!){{count(value:$v)}}","variables":{{"v":{raw}}}}}"#
            ),
        );
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&response.body).unwrap(),
            json!({"data":{"count":1}})
        );
    }
    for raw in [
        "9007199254740993.0",
        "9007199254740993e0",
        "-9007199254740993.0",
    ] {
        failure(&server, &format!("{{count(value:{raw})}}"));
        failure(&server, &format!("{{echo(value:{{nested:[{raw}]}})}}"));
        let response = server.http(
            "POST",
            &[("Content-Type", "application/json")],
            &format!(
                r#"{{"query":"query($v:Count!){{count(value:$v)}}","variables":{{"v":{raw}}}}}"#
            ),
        );
        assert_eq!(response.status, 422, "{}", response.body);
        assert!(
            serde_json::from_str::<serde_json::Value>(&response.body).unwrap()["errors"].is_array()
        );
        for default in [
            format!("Count! = {raw}"),
            format!("Data! = {{nested:[{raw}]}}"),
        ] {
            let source = format!(
                "scalar Count @representation(type:INTEGER) scalar Data @representation(type:JSON) type Query{{v(arg:{default}):Int}}"
            );
            let directory = Directory::new();
            std::fs::write(directory.0.join("bad.graphql"), source).unwrap();
            let output = directory.command(&[
                "init",
                "--schema",
                "bad.graphql",
                "--config-dir",
                "bad-project",
            ]);
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            assert!(!directory.0.join("bad.toml").exists());
        }
    }
    assert_eq!(
        data(&server, "{count echo(value:{nested:[9007199254740994e0]})}"),
        json!({"count":9007199254740994_i64,"echo":{"nested":[9007199254740994_i64]}})
    );
    assert_eq!(data(&server, "{count(value:42)}"), json!({"count":42}));
    server.stop("-TERM");
}

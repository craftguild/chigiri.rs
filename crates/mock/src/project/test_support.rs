//! Create isolated project fixtures that exercise real filesystem boundaries.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri::core::Source;

use super::generate_project;
use crate::ir::MockIr;

pub(crate) struct Fixture {
    pub base: PathBuf,
    pub project: PathBuf,
}

impl Fixture {
    pub fn new(sdl: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let parent =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/mock-project-tests");
        fs::create_dir_all(&parent).unwrap();
        let parent = parent.canonicalize().unwrap();
        let base = loop {
            let path = parent.join(format!(
                "run-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("test setup: {error}"),
            }
        };
        let fixture = Self {
            project: base.join("mock"),
            base,
        };
        // Migrate legacy fixture directives only so project tests run against the current SDL contract.
        // The product parser has no compatibility reader; schema tests cover rejection of the legacy form.
        let sdl = migrate_fixture_sdl(sdl);
        fs::write(fixture.base.join("schema.graphql"), &sdl).unwrap();
        fs::create_dir(&fixture.project).unwrap();
        fixture.write(
            "mock.luau",
            r#"--!strict
local config: ChigiriMockProjectConfig = {
    hooks = {request = {before = false, after = false}, operation = {before = false, after = false}, field = {before = false, after = false}},
    state = {fields = {}, initial = {}},
    format = "chigiri-mock-project/v10",
    schema = {sources = {"../schema.graphql"}},
    storage = {sources = {}},
    clock = {mode = "system"},
    server = {listen = "127.0.0.1:0", allowed_origins = {}},
}
return config
"#,
        );
        let ir = Arc::new(MockIr::parse(vec![Source::new("schema/001.graphql", &sdl)]).unwrap());
        for (path, bytes) in generate_project(ir).unwrap().files {
            fixture.write(&path, &bytes);
        }
        fixture
    }

    pub fn write(&self, path: &str, bytes: &str) {
        let path = self.project.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    pub fn read(&self, path: &str) -> String {
        fs::read_to_string(self.project.join(path)).unwrap()
    }
}

fn migrate_fixture_sdl(source: &str) -> String {
    if ![
        "@chigiri_mock_model",
        "@chigiri_mock_relation",
        "@chigiri_mock_inverse",
        "@chigiri_mock_resolver",
        "@chigiri_mock_crud",
        "@chigiri_mock_value",
    ]
    .iter()
    .any(|directive| source.contains(directive))
    {
        return source.to_owned();
    }
    let mut result = source
        .replace(" @chigiri_mock_model(key: \"id\")", "")
        .replace(" @chigiri_mock_model", "")
        .replace(" @chigiri_mock_resolver", "")
        .replace("@chigiri_mock_value(", "@constraint(");
    result = remove_directive_calls(&result, "@chigiri_mock_crud");
    result = result.replace("maxBytes:", "maxLength:");
    for key in [
        "authorId",
        "groupIds",
        "partnerId",
        "peerId",
        "peerRef",
        "edgeIds",
    ] {
        result = result.replace(
            &format!("@chigiri_mock_relation(key: \"{key}\")"),
            &format!("@relation(fields:[\"{key}\"], references:[\"id\"])"),
        );
        result = result.replace(
            &format!("@chigiri_mock_relation(key:\"{key}\")"),
            &format!("@relation(fields:[\"{key}\"], references:[\"id\"])"),
        );
    }
    result = replace_relation_calls(&result);
    result = result.replace("@chigiri_mock_inverse(of: \"Person.groups\")", "@relation");
    result
}

fn replace_relation_calls(source: &str) -> String {
    let mut result = source.to_owned();
    while let Some(start) = result.find("@chigiri_mock_relation") {
        let Some(key_start) = result[start..].find("key:").map(|offset| start + offset) else {
            break;
        };
        let Some(first_quote) = result[key_start + 4..]
            .find('"')
            .map(|offset| key_start + 4 + offset)
        else {
            break;
        };
        let Some(last_quote) = result[first_quote + 1..]
            .find('"')
            .map(|offset| first_quote + 1 + offset)
        else {
            break;
        };
        let key = result[first_quote + 1..last_quote].to_owned();
        let Some(end) = result[start..].find(')').map(|offset| start + offset + 1) else {
            break;
        };
        result.replace_range(
            start..end,
            &format!("@relation(fields:[\"{key}\"], references:[\"id\"])"),
        );
    }
    result
}

fn remove_directive_calls(source: &str, directive: &str) -> String {
    let mut result = source.to_owned();
    while let Some(start) = result.find(directive) {
        let open = result[start + directive.len()..]
            .find('(')
            .map(|offset| start + directive.len() + offset);
        let Some(open) = open else { break };
        let mut depth = 0usize;
        let mut end = None;
        for (offset, byte) in result.as_bytes()[open..].iter().enumerate() {
            match byte {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open + offset + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { break };
        let remove_start = result[..start].rfind(char::is_whitespace).unwrap_or(start);
        result.replace_range(remove_start..end, "");
    }
    result
}

//! Map IR field and script diagnostics back to their original TOML locations.

use std::path::Path;
use std::sync::Arc;

use chigiri_host::PortableRelativePath;
use chigiri_mock::ir::{FieldId, MockIr};
use chigiri_mock::luau::ScriptError;

use crate::diagnostic::{Diagnostic, position};

pub(crate) struct Origins {
    base: Diagnostic,
    ir: Arc<MockIr>,
    scripts: Vec<Option<(usize, usize)>>,
    paths: Option<Vec<PortableRelativePath>>,
    hooks: std::collections::BTreeMap<chigiri_mock::hooks::HookSlot, Option<(usize, usize)>>,
    consumers: std::collections::BTreeMap<String, Option<(usize, usize)>>,
}

impl Origins {
    pub fn new(
        path: &Path,
        text: &str,
        document: &toml_edit::ImDocument<String>,
        ir: Arc<MockIr>,
    ) -> Self {
        let scripts = ir
            .scripts()
            .iter()
            .map(|script| {
                let field = &ir.graph().fields()[script.field.0];
                let coordinate =
                    format!("{}.{}", ir.graph().types()[field.parent.0].name, field.name);
                document
                    .get("resolvers")
                    .and_then(|table| table.get(&coordinate))
                    .and_then(|table| table.get("luau"))
                    .and_then(|item| item.span())
                    .map(|span| position(text, span.start))
            })
            .collect();
        Self {
            base: Diagnostic::new(path, "validation", "invalid config"),
            ir,
            scripts,
            paths: None,
            consumers: document
                .get("consumers")
                .and_then(|item| item.as_table_like())
                .into_iter()
                .flat_map(|table| table.iter())
                .map(|(name, module)| {
                    let location = module
                        .get("luau")
                        .and_then(|item| item.span())
                        .map(|span| position(text, span.start));
                    (name.to_owned(), location)
                })
                .collect(),
            hooks: chigiri_mock::hooks::HookSlot::ALL
                .into_iter()
                .map(|slot| {
                    let location = document
                        .get("hooks")
                        .and_then(|item| item.get("modules"))
                        .and_then(|item| item.get(slot.name()))
                        .and_then(|item| item.get("luau"))
                        .and_then(|item| item.span())
                        .map(|span| position(text, span.start));
                    (slot, location)
                })
                .collect(),
        }
    }

    pub fn project(ir: Arc<MockIr>) -> Result<Self, Diagnostic> {
        let layout = chigiri_mock::project::Layout::new(&ir).map_err(Diagnostic::project)?;
        let mut base = Diagnostic::new(Path::new("project"), "validation", "invalid project");
        base.clear_location();
        Ok(Self {
            base,
            scripts: vec![None; ir.scripts().len()],
            paths: Some(
                layout
                    .resolvers
                    .into_iter()
                    .map(|entry| entry.path)
                    .collect(),
            ),
            ir,
            consumers: Default::default(),
            hooks: chigiri_mock::hooks::HookSlot::ALL
                .into_iter()
                .map(|slot| (slot, None))
                .collect(),
        })
    }

    pub fn script(&self, field: Option<FieldId>, error: ScriptError) -> Diagnostic {
        let mut result = self.base.clone();
        let source_position = error.source_position();
        let field = field.or_else(|| {
            error
                .script
                .and_then(|id| self.ir.scripts().get(id.0))
                .map(|script| script.field)
        });
        if let Some(field) = field.and_then(|id| self.ir.graph().fields().get(id.0)) {
            result.set_coordinate(format!(
                "{}.{}",
                self.ir.graph().types()[field.parent.0].name,
                field.name
            ));
        }
        if let Some(position) = error
            .script
            .and_then(|id| self.scripts.get(id.0))
            .copied()
            .flatten()
        {
            result.set_position(position);
        }
        if let Some(position) = source_position {
            result.set_body(position);
            if self.paths.is_none() {
                result.embedded = Some(Box::new(position));
            }
        }
        if let Some(paths) = &self.paths {
            let index = error.script.map(|id| id.0).or_else(|| {
                field.and_then(|id| self.ir.scripts().iter().position(|plan| plan.field == id))
            });
            if let Some(path) = index.and_then(|index| paths.get(index)) {
                result.set_file(path.clone());
                if let Some(position) = source_position {
                    result.set_position(position);
                }
            }
        }
        result.phase = error.phase;
        result.cause = error.cause;
        result.member.clone_from(&error.member);
        result.expected.clone_from(&error.expected);
        result.actual.clone_from(&error.actual);
        result.hint.clone_from(&error.hint);
        result.consumer.clone_from(&error.consumer);
        if let Some(name) = error
            .consumer
            .as_deref()
            .filter(|name| chigiri_mock::events::validate_name(name).is_ok())
        {
            result.clear_coordinate();
            result.set_coordinate(format!("consumer:{name}"));
            if self.paths.is_some() {
                result.set_file(format!("consumers/{name}.luau"));
                if let Some(position) = source_position {
                    result.set_position(position);
                }
            } else {
                result.clear_location();
                if let Some(location) = self.consumers.get(name).copied().flatten() {
                    result.set_position(location);
                }
                if let Some(position) = source_position {
                    result.set_body(position);
                    result.embedded = Some(Box::new(position));
                }
            }
        }
        if let Some(slot) = error.hook {
            result.clear_coordinate();
            result.set_hook_slot(slot);
            if let Some(position) = self.hooks.get(&slot).copied().flatten() {
                result.set_position(position);
            }
            if self.paths.is_some() {
                result.set_file(slot.path());
                if let Some(position) = source_position {
                    result.set_position(position);
                }
            }
        }
        result.message = error.message;
        result.exit = if error.phase == "environment" { 2 } else { 1 };
        result
    }

    pub fn coverage(&self) -> Diagnostic {
        let mut error = self.base.clone();
        error.message = "exact resolver coordinates required";
        error
    }

    /// Return only a canonical project slot path or a sanitized bundle basename.
    pub fn hook_module(&self, slot: chigiri_mock::hooks::HookSlot) -> String {
        if self.paths.is_some() {
            slot.path().into()
        } else {
            self.base.file.to_string()
        }
    }

    /// Return a resolver path from the verified layout without exposing the user's physical directory.
    pub fn resolver_module(&self, coordinate: &str) -> Option<String> {
        let index = self.ir.scripts().iter().position(|script| {
            let field = &self.ir.graph().fields()[script.field.0];
            let parent = &self.ir.graph().types()[field.parent.0];
            coordinate == format!("{}.{}", parent.name, field.name)
        })?;
        self.paths.as_ref().map_or_else(
            || Some(self.base.file.to_string()),
            |paths| paths.get(index).map(ToString::to_string),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chigiri::core::Source;
    use chigiri_mock::hooks::HookSlot;

    #[test]
    fn hook_modules_use_closed_project_paths_or_sanitized_bundle_basename() {
        let ir = Arc::new(
            MockIr::parse(vec![Source::new(
                "schema.graphql",
                "type Query { value: Int }",
            )])
            .unwrap(),
        );
        let project = Origins::project(ir.clone()).unwrap();
        let document = "".parse::<toml_edit::ImDocument<String>>().unwrap();
        let bundle = Origins::new(
            Path::new("/PRIVATE_DIRECTORY/token\u{1b}\n.toml"),
            "",
            &document,
            ir,
        );
        for slot in HookSlot::ALL {
            assert_eq!(project.hook_module(slot), slot.path());
            assert_eq!(bundle.hook_module(slot), "token.toml");
        }
    }

    #[test]
    fn ab09_resolver_modules_require_a_verified_script_in_both_input_modes() {
        let ir = Arc::new(
            MockIr::parse(vec![Source::new(
                "schema.graphql",
                "type Image { url: String } type Query { image: Image }",
            )])
            .unwrap(),
        );
        let project = Origins::project(ir.clone()).unwrap();
        let document = "".parse::<toml_edit::ImDocument<String>>().unwrap();
        let bundle = Origins::new(Path::new("mock.toml"), "", &document, ir);

        assert_eq!(
            project.resolver_module("Query.image").as_deref(),
            Some("resolvers/query/image.luau")
        );
        assert_eq!(
            bundle.resolver_module("Query.image").as_deref(),
            Some("mock.toml")
        );
        assert_eq!(project.resolver_module("Image.url"), None);
        assert_eq!(bundle.resolver_module("Image.url"), None);
    }
}

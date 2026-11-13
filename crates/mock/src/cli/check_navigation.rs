//! Derive bounded diagnostic navigation from the existing named reference registry.

use super::check_report::{ReportDiagnostic, ReportReference};
use crate::diagnostic::Diagnostic;
use crate::reference::{ReferenceDocument, Selection, Topic};

#[derive(Clone, Copy)]
pub(super) enum InputMode {
    Project,
    Bundle,
}

pub(super) struct CheckNavigation {
    document: ReferenceDocument,
    input: InputMode,
}

impl CheckNavigation {
    pub fn build(ir: &crate::ir::MockIr, input: InputMode) -> Option<Self> {
        ReferenceDocument::build(Some(ir))
            .ok()
            .map(|document| Self { document, input })
    }

    pub fn attach_report(&self, diagnostic: &mut ReportDiagnostic) {
        diagnostic.references = self.references(
            diagnostic.coordinate.as_deref(),
            diagnostic.navigation_hook.as_deref(),
            diagnostic.navigation_consumer.as_deref(),
            &diagnostic.phase,
        );
    }

    pub fn attach_text(&self, diagnostic: &mut Diagnostic) {
        let references = self.references(
            diagnostic
                .coordinate_known()
                .then_some(diagnostic.coordinate.as_ref()),
            diagnostic.hook_slot().map(|slot| slot.name()),
            diagnostic.consumer.as_deref(),
            diagnostic.phase,
        );
        diagnostic.set_reference_guidance(
            references
                .iter()
                .map(|reference| self.command(reference))
                .collect(),
            match self.input {
                InputMode::Project => "replace <PROJECT> with the project directory",
                InputMode::Bundle => "replace <BUNDLE> with the portable bundle path",
            },
        );
    }

    fn references(
        &self,
        coordinate: Option<&str>,
        hook_slot: Option<&str>,
        consumer: Option<&str>,
        phase: &str,
    ) -> Vec<ReportReference> {
        let mut references = Vec::new();
        let mut enclosing_resolver = false;
        if let Some(coordinate) = coordinate {
            enclosing_resolver = self.push(&mut references, Topic::Resolvers, coordinate);
            if let Some((parent, _)) = coordinate.split_once('.') {
                self.push(&mut references, Topic::Models, parent);
                self.push(&mut references, Topic::Types, parent);
            }
        }
        // This identity is host-owned diagnostic provenance from a resolved
        // registration. Schema-only navigation cannot invent a registry entry.
        if let Some(name) = consumer.filter(|name| {
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        }) {
            references.push(ReportReference {
                topic: "consumers".into(),
                name: name.into(),
            });
        }
        if let Some(hook_slot) = hook_slot {
            self.push(&mut references, Topic::Hooks, hook_slot);
        }
        if enclosing_resolver && phase == "business" {
            self.push(&mut references, Topic::Runtime, "errors");
        }
        references.sort();
        references.dedup();
        references.truncate(4);
        references
    }

    fn push(&self, output: &mut Vec<ReportReference>, topic: Topic, name: &str) -> bool {
        let selection = Selection::new(Some(topic), Some(name.into()))
            .expect("named navigation topic must accept names");
        if !self.document.accepts(&selection) {
            return false;
        }
        output.push(ReportReference {
            topic: topic.name().into(),
            name: name.into(),
        });
        true
    }

    fn command(&self, reference: &ReportReference) -> String {
        let input = match self.input {
            InputMode::Project => "--config-dir <PROJECT>",
            InputMode::Bundle => "--config <BUNDLE>",
        };
        format!(
            "chigiri mock reference {} {} {input}",
            reference.topic, reference.name
        )
    }
}

#[cfg(test)]
#[path = "check_navigation_tests.rs"]
mod tests;

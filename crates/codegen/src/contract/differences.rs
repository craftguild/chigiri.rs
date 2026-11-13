//! Keeps captured-tree differences and relative file names within diagnostic limits.

use std::collections::BTreeMap;

use crate::filesystem::PortableRelativePath;
use crate::{CodegenDiagnostic, CodegenError, DiagnosticReport};

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Missing,
    Extra,
    Changed,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Extra => "extra",
            Self::Changed => "changed",
        }
    }
}

#[derive(Default)]
pub(super) struct Differences {
    counts: [usize; 3],
    names: Vec<String>,
    truncated: bool,
}

impl Differences {
    pub(super) fn compare(
        expected: &BTreeMap<PortableRelativePath, Vec<u8>>,
        actual: &BTreeMap<PortableRelativePath, Vec<u8>>,
    ) -> Self {
        let mut value = Self::default();
        for (name, bytes) in expected {
            match actual.get(name) {
                None => value.add(Kind::Missing, name.as_str()),
                Some(current) if current != bytes => value.add(Kind::Changed, name.as_str()),
                Some(_) => {}
            }
        }
        for name in actual.keys().filter(|name| !expected.contains_key(*name)) {
            value.add(Kind::Extra, name.as_str());
        }
        value
    }

    pub(super) fn add(&mut self, kind: Kind, name: &str) {
        self.counts[kind as usize] += 1;
        if self.names.len() + 1 < crate::diagnostic_render::MAX_SUPPLEMENTS {
            self.names.push(format!("{}: {name}", kind.label()));
        } else {
            self.truncated = true;
        }
    }

    pub(super) fn error(self, code: &'static str, reason: &str) -> Option<CodegenError> {
        if self.counts == [0; 3] {
            return None;
        }
        let [missing, extra, changed] = self.counts;
        let mut diagnostic = CodegenDiagnostic::error(
            code,
            format!("{reason}: missing={missing}, extra={extra}, changed={changed}"),
        )
        .with_note("The path below is relative to the generated crate.");
        // The path's confidentiality and character limit are applied to the common model entrance.
        for name in self.names {
            diagnostic = diagnostic.with_note(name);
        }
        Some(CodegenError::from_report(DiagnosticReport::new(
            vec![diagnostic],
            self.truncated,
        )))
    }
}

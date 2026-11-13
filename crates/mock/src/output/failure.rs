//! Preserve fixed output failure classes without retaining returned values or unknown keys.

use crate::ir::{FieldId, MockIr, TypeRef, Wrapper};
use crate::value::ValueError;

#[derive(Clone, Copy, Debug)]
enum Class {
    Resource,
    PrivateDto,
    StoredShape,
    Provenance,
    Required,
    Type,
}

/// Sanitized output-validation failure with an optional schema-owned member.
#[derive(Debug)]
pub(crate) struct OutputError {
    source: ValueError,
    class: Class,
    member: Option<FieldId>,
}

/// Strings safe to attach to a local resolver diagnostic.
pub(crate) struct OutputDiagnostic {
    pub cause: &'static str,
    pub member: String,
    pub expected: String,
    pub actual: &'static str,
    pub hint: &'static str,
}

impl OutputError {
    pub(crate) fn is_resource_limit(&self) -> bool {
        matches!(self.class, Class::Resource)
    }

    pub(crate) fn with_resource(mut self, resource: bool) -> Self {
        if resource {
            self.class = Class::Resource;
        }
        self
    }

    pub(super) fn with_member(mut self, member: FieldId) -> Self {
        if self.member.is_none() {
            self.member = Some(member);
        }
        self
    }

    pub(super) fn stored(mut self) -> Self {
        if !matches!(self.class, Class::Resource | Class::Provenance) {
            self.class = Class::StoredShape;
        }
        self
    }

    pub(crate) fn into_value(self) -> ValueError {
        self.source
    }

    pub(crate) fn diagnostic(&self, ir: &MockIr, root: FieldId) -> OutputDiagnostic {
        let member = self.member.unwrap_or(root);
        let field = &ir.graph().fields()[member.0];
        let parent = &ir.graph().types()[field.parent.0].name;
        let member_name = format!("{parent}.{}", field.name);
        let expected_type = type_reference(ir, &field.ty);
        let (cause, expected, actual, hint) = match self.class {
            Class::Resource => (
                "MOCK_RESOURCE_LIMIT",
                "bounded output validation".into(),
                "resource limit reached",
                "Reduce the output size or complexity.",
            ),
            Class::PrivateDto => (
                "MOCK_OUTPUT_PRIVATE_FIELD",
                format!("public resolver DTO fields for {parent}"),
                "private field supplied by resolver DTO",
                "Return only public stored fields from an arbitrary DTO.",
            ),
            Class::StoredShape => (
                "MOCK_STORED_VALUE_SHAPE",
                format!("stored value compatible with {expected_type}"),
                "invalid stored value shape",
                "Correct the stored value to match the generated storage contract.",
            ),
            Class::Provenance => (
                "MOCK_RECORD_PROVENANCE",
                format!("unchanged authoritative record compatible with {expected_type}"),
                "modified, forged, foreign, or stale record",
                "Return the authoritative record unchanged or construct a valid DTO.",
            ),
            Class::Required => (
                "MOCK_OUTPUT_REQUIRED_FIELD",
                format!("required output member compatible with {expected_type}"),
                "required output member missing",
                "Return every required stored output member.",
            ),
            Class::Type => (
                "MOCK_OUTPUT_TYPE_MISMATCH",
                format!("output value compatible with {expected_type}"),
                actual(&self.source),
                "Return a value compatible with the generated output contract.",
            ),
        };
        OutputDiagnostic {
            cause,
            member: member_name,
            expected,
            actual,
            hint,
        }
    }
}

impl From<ValueError> for OutputError {
    fn from(source: ValueError) -> Self {
        let class = match source.0 {
            "private output DTO field" => Class::PrivateDto,
            "required output DTO field missing" | "required stored output field missing" => {
                Class::Required
            }
            "output record absent or modified"
            | "foreign output record provenance"
            | "stale output record provenance"
            | "output record modified"
            | "output record provenance required" => Class::Provenance,
            _ => Class::Type,
        };
        Self {
            source,
            class,
            member: None,
        }
    }
}

fn actual(error: &ValueError) -> &'static str {
    match error.0 {
        "null for non-null output" => "null",
        "output list required" => "non-list value",
        "output object required" => "non-object value",
        "abstract output requires __typename" => "abstract object without typename",
        "unknown concrete output type" | "output type is not a possible member" => {
            "incompatible concrete output type"
        }
        _ => "incompatible output value",
    }
}

fn type_reference(ir: &MockIr, reference: &TypeRef) -> String {
    let mut output = String::new();
    for wrapper in reference.wrappers.iter() {
        if *wrapper == Wrapper::List {
            output.push('[');
        }
    }
    output.push_str(&ir.graph().types()[reference.named.0].name);
    for wrapper in reference.wrappers.iter().rev() {
        output.push(match wrapper {
            Wrapper::NonNull => '!',
            Wrapper::List => ']',
        });
    }
    output
}

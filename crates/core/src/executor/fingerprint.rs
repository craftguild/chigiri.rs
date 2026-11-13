//! Stable hash of semantic schema shape for generated typed-adapter ABI checks.

use crate::language::ast::{TypeRef, TypeRefKind, Value as AstValue, ValueNode};
use crate::schema::{SemanticSchema, TypeKind};

/// ABI fingerprint shared by the generated resolver table and runtime descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolverSchemaFingerprint {
    component_count: usize,
    hash: u64,
}

impl ResolverSchemaFingerprint {
    /// Inserts a fixed value from generated source.
    #[doc(hidden)]
    pub const fn new(component_count: usize, hash: u64) -> Self {
        Self {
            component_count,
            hash,
        }
    }

    /// Returns the number of structural elements written to the fingerprint.
    #[doc(hidden)]
    pub const fn component_count(self) -> usize {
        self.component_count
    }

    /// Returns the stable hash value.
    #[doc(hidden)]
    pub const fn hash(self) -> u64 {
        self.hash
    }
}

/// The codegen and runtime builder calculate the same generated adapter ABI.
#[doc(hidden)]
pub fn generated_resolver_abi_fingerprint(schema: &SemanticSchema) -> ResolverSchemaFingerprint {
    let mut state = Fingerprint::new();
    roots(&mut state, schema);
    let types = schema
        .types()
        .iter()
        .filter(|ty| ty.name_span().is_some())
        .collect::<Vec<_>>();
    state.usize(types.len());
    for ty in types {
        state.tag(1);
        state.tag(type_kind(ty.kind()));
        state.text(ty.name());
        fields(&mut state, ty.fields());
        names(
            &mut state,
            ty.implemented_interfaces()
                .iter()
                .map(|name| name.value.as_str()),
        );
        possible_types(&mut state, ty);
        names(
            &mut state,
            ty.enum_values()
                .iter()
                .map(|value| value.name.value.as_str()),
        );
        inputs(&mut state, ty.input_fields());
        state.boolean(
            ty.directives()
                .iter()
                .any(|directive| directive.name.value == "oneOf"),
        );
    }
    state.finish()
}

fn roots(state: &mut Fingerprint, schema: &SemanticSchema) {
    for root in [
        schema.roots().query(),
        schema.roots().mutation(),
        schema.roots().subscription(),
    ] {
        match root {
            Some(root) => {
                state.tag(1);
                state.text(&root.type_name);
            }
            None => state.tag(0),
        }
    }
}

fn fields(state: &mut Fingerprint, fields: &[crate::language::ast::FieldDefinition]) {
    state.usize(fields.len());
    for field in fields {
        state.text(&field.name.value);
        type_ref(state, &field.ty);
        inputs(state, &field.arguments);
    }
}

fn inputs(state: &mut Fingerprint, inputs: &[crate::language::ast::InputValueDefinition]) {
    state.usize(inputs.len());
    for input in inputs {
        state.text(&input.name.value);
        type_ref(state, &input.ty);
        match &input.default_value {
            Some(value) => {
                state.tag(1);
                value_node(state, value);
            }
            None => state.tag(0),
        }
    }
}

fn possible_types(state: &mut Fingerprint, ty: &crate::schema::SemanticType) {
    match ty.kind() {
        TypeKind::Union => names(
            state,
            ty.union_members()
                .iter()
                .map(|member| member.value.as_str()),
        ),
        // The interface membership has hashed the object side implemented_interfaces as a normal expression.
        _ => state.usize(0),
    }
}

fn names<'a>(state: &mut Fingerprint, values: impl IntoIterator<Item = &'a str>) {
    let values = values.into_iter().collect::<Vec<_>>();
    state.usize(values.len());
    for value in values {
        state.text(value);
    }
}

fn type_ref(state: &mut Fingerprint, reference: &TypeRef) {
    let mut work = vec![reference];
    while let Some(reference) = work.pop() {
        match &reference.kind {
            TypeRefKind::Named(name) => {
                state.tag(1);
                state.text(&name.value);
            }
            TypeRefKind::List(inner) => {
                state.tag(2);
                work.push(inner);
            }
            TypeRefKind::NonNull(inner) => {
                state.tag(3);
                work.push(inner);
            }
        }
    }
}

fn value_node(state: &mut Fingerprint, root: &ValueNode) {
    enum Task<'a> {
        Value(&'a ValueNode),
        Name(&'a str),
    }
    let mut work = vec![Task::Value(root)];
    while let Some(task) = work.pop() {
        let Task::Value(value) = task else {
            let Task::Name(name) = task else {
                unreachable!();
            };
            state.text(name);
            continue;
        };
        match &value.value {
            AstValue::Variable(name) => scalar(state, 1, &name.value),
            AstValue::Int(value) => scalar(state, 2, value),
            AstValue::Float(value) => scalar(state, 3, value),
            AstValue::String(value) => scalar(state, 4, &value.value),
            AstValue::Boolean(value) => {
                state.tag(5);
                state.boolean(*value);
            }
            AstValue::Null => state.tag(6),
            AstValue::Enum(name) => scalar(state, 7, &name.value),
            AstValue::List(values) => {
                state.tag(8);
                state.usize(values.len());
                work.extend(values.iter().rev().map(Task::Value));
            }
            AstValue::Object(fields) => {
                state.tag(9);
                state.usize(fields.len());
                for field in fields.iter().rev() {
                    work.push(Task::Value(&field.value));
                    work.push(Task::Name(&field.name.value));
                }
            }
        }
    }
}

fn scalar(state: &mut Fingerprint, tag: u8, value: &str) {
    state.tag(tag);
    state.text(value);
}

const fn type_kind(kind: TypeKind) -> u8 {
    match kind {
        TypeKind::Scalar => 1,
        TypeKind::Object => 2,
        TypeKind::Interface => 3,
        TypeKind::Union => 4,
        TypeKind::Enum => 5,
        TypeKind::InputObject => 6,
    }
}

struct Fingerprint {
    component_count: usize,
    hash: u64,
}

impl Fingerprint {
    const fn new() -> Self {
        Self {
            component_count: 0,
            hash: 0xcbf2_9ce4_8422_2325,
        }
    }

    fn tag(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn boolean(&mut self, value: bool) {
        self.tag(u8::from(value));
    }

    fn usize(&mut self, value: usize) {
        self.bytes(&(value as u64).to_le_bytes());
    }

    fn text(&mut self, value: &str) {
        self.usize(value.len());
        self.bytes(value.as_bytes());
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.component_count = self.component_count.saturating_add(1);
        for byte in bytes {
            self.hash ^= u64::from(*byte);
            self.hash = self.hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    const fn finish(self) -> ResolverSchemaFingerprint {
        ResolverSchemaFingerprint::new(self.component_count, self.hash)
    }
}

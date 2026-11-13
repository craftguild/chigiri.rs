//! Generated ABI types shared by generated code and the runtime.

mod list;

use chigiri_core::executable::ScalarCoercionError;
use chigiri_core::{FieldResult, Value};

pub use chigiri_core::executor::FieldContext;
pub use list::{DescriptorList, DescriptorListIter};

/// ABI version of the private generated-code surface.
pub const GENERATED_ABI_VERSION: u32 = 8;

/// Nullable input distinguishes an omitted position from an explicit null.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Input<T> {
    /// The value is not specified.
    Undefined,
    /// Explicit null.
    Null,
    /// A non-null value was provided.
    Value(T),
}

/// Input/output conversion contract for an application-specific scalar.
pub trait Scalar: Send + Sync + 'static + Sized {
    /// Converts GraphQL input to the domain scalar.
    fn parse(value: Value) -> Result<Self, ScalarCoercionError>;

    /// Converts the domain scalar to GraphQL output.
    fn serialize(&self) -> FieldResult<Value>;
}

/// GraphQL named-type kind in a static descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypeDescriptorKind {
    /// Scalar type.
    Scalar,
    /// Object type.
    Object,
    /// Interface type.
    Interface,
    /// Union type.
    Union,
    /// Enum type.
    Enum,
    /// Input object type.
    InputObject,
}

/// Schema root-operation names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationRootsDescriptor {
    /// The query root name.
    pub query: &'static str,
    /// The name of the mutation root.
    pub mutation: Option<&'static str>,
    /// The subscription root name, when the schema declares one.
    pub subscription: Option<&'static str>,
}

/// A directive application's argument value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectiveArgumentDescriptor {
    /// The name of the argument.
    pub name: &'static str,
    /// Canonical GraphQL literal.
    pub value: &'static str,
    /// Parse-free validated constant value.
    pub value_descriptor: ConstValueDescriptor,
}

/// Static GraphQL type-reference descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypeRefDescriptor {
    /// Named type.
    Named(&'static str),
    /// List item type.
    List(&'static TypeRefDescriptor),
    /// The inner type of a non-null wrapper.
    NonNull(&'static TypeRefDescriptor),
}

/// Descriptor for a constant object field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConstObjectFieldDescriptor {
    /// GraphQL field name.
    pub name: &'static str,
    /// Field value.
    pub value: ConstValueDescriptor,
}

/// Parse-free GraphQL constant-value descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConstValueDescriptor {
    /// Integer lexeme.
    Int(&'static str),
    /// Floating-point lexeme.
    Float(&'static str),
    /// Decoded string value.
    String(&'static str),
    /// Boolean value.
    Boolean(bool),
    /// Null value.
    Null,
    /// Enum value name.
    Enum(&'static str),
    /// List item in source order.
    List(DescriptorList<ConstValueDescriptor>),
    /// Object fields in source order.
    Object(DescriptorList<ConstObjectFieldDescriptor>),
}

/// Type-system directive application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppliedDirectiveDescriptor {
    /// The name of the directive except `@`.
    pub name: &'static str,
    /// Arguments in source order.
    pub arguments: DescriptorList<DirectiveArgumentDescriptor>,
}

/// Field-argument or input-field descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputValueDescriptor {
    /// GraphQL name.
    pub name: &'static str,
    /// SDL description.
    pub description: Option<&'static str>,
    /// Canonical GraphQL type reference.
    pub type_ref: &'static str,
    /// Parse-free validated type reference.
    pub type_descriptor: TypeRefDescriptor,
    /// Canonical default literal.
    pub default_value: Option<&'static str>,
    /// Parse-free canonical default literal.
    pub default_descriptor: Option<ConstValueDescriptor>,
    /// Directive applications in source order.
    pub directives: DescriptorList<AppliedDirectiveDescriptor>,
    /// The reason for `@deprecated`.
    pub deprecation_reason: Option<&'static str>,
}

/// Descriptor of the output field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FieldDescriptor {
    /// GraphQL name.
    pub name: &'static str,
    /// SDL description.
    pub description: Option<&'static str>,
    /// Canonical GraphQL type reference.
    pub type_ref: &'static str,
    /// Parse-free validated type reference.
    pub type_descriptor: TypeRefDescriptor,
    /// Argument definitions in source order.
    pub arguments: DescriptorList<InputValueDescriptor>,
    /// Directive applications in source order.
    pub directives: DescriptorList<AppliedDirectiveDescriptor>,
    /// The reason for `@deprecated`.
    pub deprecation_reason: Option<&'static str>,
}

/// Descriptor for an enum value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnumValueDescriptor {
    /// GraphQL name.
    pub name: &'static str,
    /// SDL description.
    pub description: Option<&'static str>,
    /// Directive applications in source order.
    pub directives: DescriptorList<AppliedDirectiveDescriptor>,
    /// The reason for `@deprecated`.
    pub deprecation_reason: Option<&'static str>,
}

/// Named type in a schema descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TypeDescriptor {
    /// GraphQL name.
    pub name: &'static str,
    /// Type kind.
    pub kind: TypeDescriptorKind,
    /// SDL description.
    pub description: Option<&'static str>,
    /// Object or interface fields.
    pub fields: DescriptorList<FieldDescriptor>,
    /// Names of implemented interfaces.
    pub interfaces: DescriptorList<&'static str>,
    /// Union member or interface possible object name.
    pub possible_types: DescriptorList<&'static str>,
    /// Enum values.
    pub enum_values: DescriptorList<EnumValueDescriptor>,
    /// Input object fields.
    pub input_fields: DescriptorList<InputValueDescriptor>,
    /// Directive applications in source order.
    pub directives: DescriptorList<AppliedDirectiveDescriptor>,
    /// Whether this is a one-of input object.
    pub is_one_of: bool,
    /// Custom scalar `@specifiedBy(url:)` value.
    pub specified_by_url: Option<&'static str>,
}

/// Descriptor for a directive definition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectiveDefinitionDescriptor {
    /// GraphQL name except for `@`.
    pub name: &'static str,
    /// SDL description.
    pub description: Option<&'static str>,
    /// Argument definitions in source order.
    pub arguments: DescriptorList<InputValueDescriptor>,
    /// Whether the directive is repeatable.
    pub repeatable: bool,
    /// Canonical location name.
    pub locations: DescriptorList<&'static str>,
}

/// Descriptor that overlays source-extension metadata onto a core-owned
/// built-in scalar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltInScalarOverlayDescriptor {
    /// Built-in scalar names.
    pub name: &'static str,
    /// Source-order directive applications on the extension.
    pub directives: DescriptorList<AppliedDirectiveDescriptor>,
}

/// Schema descriptor used by the runtime without reparsing SDL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchemaDescriptor {
    /// Generated-code ABI version.
    pub abi_version: u32,
    /// Canonical SHA-256 of the input schema source set.
    pub schema_digest: chigiri_core::SchemaDigest,
    /// Description of the explicit schema definition.
    pub description: Option<&'static str>,
    /// Directive applications on the schema definition and extensions, in source order.
    pub schema_directives: DescriptorList<AppliedDirectiveDescriptor>,
    /// Operation root name.
    pub roots: OperationRootsDescriptor,
    /// Type descriptors in source order; excludes built-in scalars.
    pub types: DescriptorList<TypeDescriptor>,
    /// Scalar-extension metadata in specified-built-in order.
    pub built_in_scalar_overlays: DescriptorList<BuiltInScalarOverlayDescriptor>,
    /// Source-order custom directive definitions; built-ins are omitted.
    pub directives: DescriptorList<DirectiveDefinitionDescriptor>,
}

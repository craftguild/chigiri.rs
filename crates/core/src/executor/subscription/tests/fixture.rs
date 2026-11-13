use std::sync::Arc;

use crate::executable::{
    CoercedVariables, CustomScalarRegistry, ValidatedDocument, coerce_variable_values,
    validate_executable_document,
};
use crate::executor::{Plugin, PreparedSubscriptionOperation, SubscriptionExecutionInput};
use crate::language::{parse_executable_document, parse_type_system_document};
use crate::schema::{SemanticSchema, compose_schema};
use crate::{
    DocumentDigest, ExecutionLimits, LifecycleObserver, Object, OperationIdentity, OperationKey,
    SchemaDigest, Source, SourceId,
};

use super::support::{TestContext, TestDispatch};

const OPERATION: &str = r#"
    subscription Events($id: ID!) @audit(level: 1) {
      ...RootEvent
    }
    fragment RootEvent on Subscription {
      chosen: event(id: $id) @audit(level: 2) { required label }
    }
"#;

pub(super) struct Fixture {
    pub schema: Arc<SemanticSchema>,
    pub document: Arc<ValidatedDocument>,
    pub variables: Arc<CoercedVariables>,
    pub scalars: Arc<CustomScalarRegistry>,
    pub source: Arc<Source>,
    pub identity: Arc<OperationIdentity>,
}

impl Fixture {
    pub fn new() -> Self {
        let limits = ExecutionLimits::unbounded();
        let parsed_schema = parse_type_system_document(
            SourceId::new(800),
            r#"
                schema { query: Query subscription: Subscription }
                scalar Payload
                directive @audit(level: Int!) on SUBSCRIPTION | FIELD
                type Query { ping: String nested: Subscription! }
                type Subscription {
                  event(id: ID!): Event!
                  maybe: Event
                  items: [Event]!
                  node: Node!
                  payload: Payload!
                }
                interface Node { label: String }
                type Event implements Node { required: String! label: String }
            "#,
            &limits,
        );
        assert!(parsed_schema.is_valid());
        let schema = compose_schema(&[parsed_schema.document.unwrap()], &limits)
            .schema
            .unwrap();
        let parsed = parse_executable_document(SourceId::new(801), OPERATION, &limits);
        assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
        let document = validate_executable_document(&schema, &parsed.document.unwrap(), &limits)
            .document
            .unwrap();
        let selected = document.select_operation(None).unwrap();
        let mut raw = Object::new();
        raw.insert("id", "room");
        let scalars = CustomScalarRegistry::new();
        let variables = coerce_variable_values(
            &schema,
            &selected.operation.variable_definitions,
            raw,
            &scalars,
            &limits,
        )
        .variables
        .unwrap();
        Self {
            schema: Arc::new(schema),
            document: Arc::new(document),
            variables: Arc::new(variables),
            scalars: Arc::new(scalars),
            source: Arc::new(Source::new("subscription.graphql", OPERATION)),
            identity: Arc::new(OperationIdentity::new(
                SchemaDigest::from_bytes([1; 32]),
                OperationKey::new(DocumentDigest::from_bytes([2; 32]), None),
            )),
        }
    }

    pub fn operation(&self, limits: ExecutionLimits) -> PreparedSubscriptionOperation {
        PreparedSubscriptionOperation::new(
            Arc::clone(&self.schema),
            Arc::clone(&self.document),
            None,
            Arc::clone(&self.variables),
            Arc::clone(&self.scalars),
            Arc::clone(&self.source),
            limits,
            false,
            Arc::clone(&self.identity),
            None,
        )
        .unwrap()
    }

    pub fn operation_for(
        &self,
        query: &str,
        raw: Object,
        limits: ExecutionLimits,
    ) -> PreparedSubscriptionOperation {
        let parsed = parse_executable_document(SourceId::new(803), query, &limits);
        assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
        let document =
            validate_executable_document(&self.schema, &parsed.document.unwrap(), &limits)
                .document
                .unwrap();
        let selected = document.select_operation(None).unwrap();
        let variables = coerce_variable_values(
            &self.schema,
            &selected.operation.variable_definitions,
            raw,
            &self.scalars,
            &limits,
        )
        .variables
        .unwrap();
        PreparedSubscriptionOperation::new(
            Arc::clone(&self.schema),
            Arc::new(document),
            None,
            Arc::new(variables),
            Arc::clone(&self.scalars),
            Arc::new(Source::new("alternate.graphql", query)),
            limits,
            false,
            Arc::clone(&self.identity),
            None,
        )
        .unwrap()
    }

    pub fn input<'a>(
        &'a self,
        dispatch: &'a TestDispatch,
        plugins: &'a [Arc<dyn Plugin<TestContext>>],
        context: &'a TestContext,
        lifecycle: Option<&'a dyn LifecycleObserver>,
    ) -> SubscriptionExecutionInput<'a, TestContext> {
        SubscriptionExecutionInput::new(
            dispatch, plugins, context, None, lifecycle, None, None, None,
        )
    }
}

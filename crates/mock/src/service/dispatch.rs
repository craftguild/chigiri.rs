//! Bind a worker client and IR field dispatch to an owned production endpoint.

mod factory;
mod subscription;
#[cfg(test)]
mod subscription_tests;

use std::sync::Arc;

use chigiri::core::Object;
use chigiri::core::executable::CustomScalarRegistry;
use chigiri::core::executor::{ArgumentValues, ResolvedObject, ResolvedValue};
use chigiri::core::schema::TypeKind;
use chigiri::experimental::{OwnedResolverDispatch, OwnedSchema};
use chigiri::{BoxFuture, Endpoint, FieldError, FieldResult};

use super::Client;
use crate::ir::{FieldId, MockIr};
use crate::luau::ScriptError;
use crate::output::{OutputObject, OutputValue};
use factory::{Factory, Scopes};

type Report = dyn Fn(FieldId, ScriptError) -> FieldError + Send + Sync;

struct Dispatch {
    ir: Arc<MockIr>,
    coordinates: Vec<(Arc<str>, Arc<str>)>,
    report: Arc<Report>,
}

/// Build an endpoint with production limits and the fixed operation deadline.
/// Setting `graphiql` to true explicitly exposes the UI and schema introspection.
pub fn endpoint(
    client: Client,
    graphiql: bool,
    report: impl Fn(FieldId, ScriptError) -> FieldError + Send + Sync + 'static,
) -> Result<Endpoint<Client>, ScriptError> {
    build(
        client,
        graphiql,
        report,
        None,
        None,
        super::hooks::Hooks,
        None,
    )
}

/// Builds an endpoint with a fresh host observation scope for each streaming phase.
///
/// The factory supplies listener metadata and shared diagnostic sinks, never a
/// reused request budget or state. Finite HTTP requests keep their ambient scope.
pub fn endpoint_with_scopes(
    client: Client,
    graphiql: bool,
    report: impl Fn(FieldId, ScriptError) -> FieldError + Send + Sync + 'static,
    scopes: impl Fn() -> super::HttpScope + Send + Sync + 'static,
) -> Result<Endpoint<Client>, ScriptError> {
    build(
        client,
        graphiql,
        report,
        None,
        None,
        super::hooks::Hooks,
        Some(Scopes {
            create: Arc::new(scopes),
            http: None,
        }),
    )
}

pub(crate) fn managed_endpoint(
    client: Client,
    graphiql: bool,
    report: impl Fn(FieldId, ScriptError) -> FieldError + Send + Sync + 'static,
    create: impl Fn() -> super::HttpScope + Send + Sync + 'static,
    http: Arc<super::HttpScopes>,
) -> Result<Endpoint<Client>, ScriptError> {
    build(
        client,
        graphiql,
        report,
        None,
        None,
        super::hooks::Hooks,
        Some(Scopes {
            create: Arc::new(create),
            http: Some(http),
        }),
    )
}

fn build(
    client: Client,
    graphiql: bool,
    report: impl Fn(FieldId, ScriptError) -> FieldError + Send + Sync + 'static,
    response_limit: Option<usize>,
    output_nodes: Option<usize>,
    plugin: impl chigiri::Plugin<Client>,
    scopes: Option<Scopes>,
) -> Result<Endpoint<Client>, ScriptError> {
    let ir = client.ir().clone();
    let coordinates = ir
        .public_graph()
        .fields()
        .iter()
        .filter_map(|field| {
            let parent = &ir.public_graph().types()[field.parent.0];
            (parent.kind == TypeKind::Object).then(|| (parent.name.clone(), field.name.clone()))
        })
        .collect();
    let input = ir
        .public_graph()
        .source()
        .clone()
        .bind_with_subscriptions(Dispatch {
            ir,
            coordinates,
            report: Arc::new(report),
        })
        .map_err(|_| ScriptError::new("contract", "cannot bind owned dispatch"))?;
    let observer =
        super::hooks::Observe::new(scopes.as_ref().and_then(|scopes| scopes.http.clone()));
    let mut builder = Endpoint::builder(input)
        .graphiql(graphiql)
        .context_factory(Factory(client, scopes))
        .push_plugin(plugin)
        .execution_timeout(crate::limits::OPERATION_TIME)
        .inject(|components| {
            components.observability().error_observer(observer.clone());
            components.observability().lifecycle_observer(observer);
            components
                .http()
                .overall_timeout(crate::limits::OPERATION_TIME);
        });
    if let Some(bytes) = response_limit {
        builder = builder.response_byte_limit(bytes);
    }
    if let Some(nodes) = output_nodes {
        let mut limits = chigiri::ExecutionLimits::production();
        limits.max_output_nodes = nodes;
        builder = builder.execution_limits(limits);
    }
    builder
        .build()
        .map_err(|_| ScriptError::new("environment", "cannot build owned endpoint"))
}

#[cfg(test)]
pub(super) fn limited_for_test(client: Client, bytes: usize) -> Endpoint<Client> {
    build(
        client,
        false,
        |_, _| FieldError::public("Resolver failed"),
        Some(bytes),
        None,
        super::hooks::Hooks,
        None,
    )
    .unwrap()
}

#[cfg(test)]
pub(super) fn limited_nodes_for_test(client: Client, nodes: usize) -> Endpoint<Client> {
    build(
        client,
        false,
        |_, _| FieldError::public("Resolver failed"),
        None,
        Some(nodes),
        super::hooks::Hooks,
        None,
    )
    .unwrap()
}

/// Build the production dispatch path while letting a test plugin control hook scheduling.
#[cfg(test)]
pub(super) fn scheduled_for_test(
    client: Client,
    plugin: impl chigiri::Plugin<Client>,
) -> Endpoint<Client> {
    build(
        client,
        true,
        |_, _| FieldError::public("Resolver failed").code("MOCK_RESOLVER_ERROR"),
        None,
        None,
        plugin,
        None,
    )
    .unwrap()
}

impl OwnedResolverDispatch<Client> for Dispatch {
    fn schema(&self) -> &OwnedSchema {
        self.ir.public_graph().source()
    }
    fn coordinates(&self) -> &[(Arc<str>, Arc<str>)] {
        &self.coordinates
    }
    fn register_custom_scalars(&self, registry: &mut CustomScalarRegistry) {
        *registry = crate::value::public_scalar_registry(&self.ir);
    }
    fn resolve<'a>(
        &'a self,
        client: &'a Client,
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            let id = self.field(client, parent_type, field_name)?;
            let arguments = self.arguments(id, arguments)?;
            let parent = parent
                .map(|parent| {
                    parent
                        .downcast_ref::<OutputObject>()
                        .cloned()
                        .ok_or_else(contract)
                })
                .transpose()?;
            let output = super::hooks::resolve(client, id, arguments, parent)
                .await
                .map_err(|error| self.map_error(id, error))?;
            convert(self.schema(), output)
        })
    }
}

impl Dispatch {
    fn field(&self, client: &Client, parent_type: &str, field_name: &str) -> FieldResult<FieldId> {
        if !Arc::ptr_eq(&self.ir, client.ir()) {
            return Err(contract());
        }
        let graph = self.ir.graph();
        let ty = graph.type_id(parent_type).ok_or_else(contract)?;
        graph.types()[ty.0]
            .fields
            .get(field_name)
            .copied()
            .ok_or_else(contract)
    }

    fn arguments(&self, id: FieldId, arguments: ArgumentValues) -> FieldResult<Object> {
        let graph = self.ir.graph();
        let field = &graph.fields()[id.0];
        let values = arguments.into_values();
        if values.len() != field.arguments.len() {
            return Err(contract());
        }
        let mut arguments = Object::with_capacity(values.len());
        for (id, value) in field.arguments.iter().zip(values) {
            if let Some(value) = value.into_value() {
                arguments.insert(graph.inputs()[id.0].name.to_string(), value);
            }
        }
        Ok(arguments)
    }

    fn map_error(&self, field: FieldId, error: ScriptError) -> FieldError {
        if error.phase == "business" {
            FieldError::public("Request failed").code("BUSINESS_REJECTION")
        } else {
            (self.report)(field, error)
        }
    }
}

fn convert(schema: &OwnedSchema, output: OutputValue) -> FieldResult<ResolvedValue> {
    Ok(match output {
        OutputValue::Null => ResolvedValue::Null,
        OutputValue::Scalar(value) => ResolvedValue::Leaf(value),
        OutputValue::List(values) => ResolvedValue::List(
            values
                .into_iter()
                .map(|value| convert(schema, value))
                .collect(),
        ),
        OutputValue::Object(object) => {
            if !schema.same_instance(object.snapshot().ir().public_graph().source()) {
                return Err(contract());
            }
            let name = object.snapshot().ir().graph().types()[object.ty().0]
                .name
                .clone();
            ResolvedValue::Object(schema.object(&name, object).map_err(|_| contract())?)
        }
    })
}

fn contract() -> FieldError {
    FieldError::public("mock dispatch contract mismatch").code("MOCK_RESOLVER_ERROR")
}

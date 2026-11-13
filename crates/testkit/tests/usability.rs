//! Application-facing usability contracts from ADR-0016 through ADR-0019.

use ::generated;
#[path = "usability/support.rs"]
mod support;
#[path = "usability/timeout_parity.rs"]
mod timeout_parity;

#[path = "usability/built_schema_graph.rs"]
mod built_schema_graph;
#[path = "usability/built_schema_rate.rs"]
mod built_schema_rate;
#[path = "usability/configuration_bounds.rs"]
mod configuration_bounds;
#[path = "usability/configuration_diagnostics.rs"]
mod configuration_diagnostics;
#[path = "usability/context_and_bridges.rs"]
mod context_and_bridges;
#[path = "usability/distributed_tracing.rs"]
mod distributed_tracing;
#[path = "usability/endpoint_builder.rs"]
mod endpoint_builder;
#[cfg(feature = "graphiql")]
#[path = "usability/graphiql_bridges.rs"]
mod graphiql_bridges;
#[cfg(feature = "graphiql")]
#[path = "usability/graphiql_configuration.rs"]
mod graphiql_configuration;
#[cfg(feature = "graphiql")]
#[path = "usability/graphiql_policy.rs"]
mod graphiql_policy;
#[cfg(feature = "graphiql")]
#[path = "usability/graphiql_response.rs"]
mod graphiql_response;
#[cfg(feature = "graphiql")]
#[path = "usability/graphiql_wire.rs"]
mod graphiql_wire;
#[path = "usability/injection_equivalence.rs"]
mod injection_equivalence;
#[path = "usability/injection_support.rs"]
mod injection_support;
#[path = "usability/instrumentation_registration.rs"]
mod instrumentation_registration;
#[path = "usability/projection_panic.rs"]
mod projection_panic;
#[path = "usability/rate.rs"]
mod rate;
#[path = "usability/rate_configuration.rs"]
mod rate_configuration;
#[path = "usability/rate_memory.rs"]
mod rate_memory;
#[path = "usability/rate_support.rs"]
mod rate_support;
#[path = "usability/request_identity.rs"]
mod request_identity;
#[path = "usability/response_hook_graph.rs"]
mod response_hook_graph;
#[path = "usability/subscription_configuration/mod.rs"]
mod subscription_configuration;
#[path = "usability/subscription_sizing.rs"]
mod subscription_sizing;
#[path = "usability/surface.rs"]
mod surface;

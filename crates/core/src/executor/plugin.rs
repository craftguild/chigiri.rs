//! Three-phase consuming middleware chain for requests, operations, and fields.

use std::sync::Arc;

use crate::{FieldResult, GraphQlResponse};

use super::{BoxFuture, FieldContext, OperationContext, RequestContext, ResolvedValue};

#[cfg(test)]
mod tests;

/// Runtime-neutral plugin spanning request, operation, and field phases.
pub trait Plugin<C: 'static>: Send + Sync + 'static {
    /// Intercepts a request before parsing and cache lookup.
    fn request<'a>(
        &'a self,
        context: RequestContext<'a, C>,
        next: RequestNext<'a, C>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        next.run(context)
    }

    /// Wrap the operation after validation, selection, and coercion.
    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, C>,
        next: OperationNext<'a, C>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        next.run(context)
    }

    /// Wraps a field immediately before resolver dispatch.
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, C>,
        next: FieldNext<'a, C>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        next.run(context)
    }
}

type RequestTerminal<'a, C> =
    Box<dyn FnOnce(RequestContext<'a, C>) -> BoxFuture<'a, GraphQlResponse> + Send + 'a>;
type OperationTerminal<'a, C> =
    Box<dyn FnOnce(OperationContext<'a, C>) -> BoxFuture<'a, GraphQlResponse> + Send + 'a>;
type FieldTerminal<'a, C> =
    Box<dyn FnOnce(FieldContext<'a, C>) -> BoxFuture<'a, FieldResult<ResolvedValue>> + Send + 'a>;

/// Runs the remaining request plugins or terminal exactly once.
pub struct RequestNext<'a, C> {
    plugins: &'a [Arc<dyn Plugin<C>>],
    terminal: RequestTerminal<'a, C>,
}

impl<'a, C: 'a + 'static> RequestNext<'a, C> {
    /// The middleware engine will create a request chain.
    #[doc(hidden)]
    pub fn new(
        plugins: &'a [Arc<dyn Plugin<C>>],
        terminal: impl FnOnce(RequestContext<'a, C>) -> BoxFuture<'a, GraphQlResponse> + Send + 'a,
    ) -> Self {
        Self {
            plugins,
            terminal: Box::new(terminal),
        }
    }

    /// Passes the context to the next plugin or terminal.
    pub fn run(self, context: RequestContext<'a, C>) -> BoxFuture<'a, GraphQlResponse> {
        let Self { plugins, terminal } = self;
        if let Some((plugin, remaining)) = plugins.split_first() {
            plugin.request(
                context,
                Self {
                    plugins: remaining,
                    terminal,
                },
            )
        } else {
            terminal(context)
        }
    }
}

/// Runs the remaining operation plugins or terminal exactly once.
pub struct OperationNext<'a, C> {
    plugins: &'a [Arc<dyn Plugin<C>>],
    terminal: OperationTerminal<'a, C>,
}

impl<'a, C: 'a + 'static> OperationNext<'a, C> {
    /// The middleware engine will create the operation chain.
    #[doc(hidden)]
    pub fn new(
        plugins: &'a [Arc<dyn Plugin<C>>],
        terminal: impl FnOnce(OperationContext<'a, C>) -> BoxFuture<'a, GraphQlResponse> + Send + 'a,
    ) -> Self {
        Self {
            plugins,
            terminal: Box::new(terminal),
        }
    }

    /// Passes the context to the next plugin or terminal.
    pub fn run(self, context: OperationContext<'a, C>) -> BoxFuture<'a, GraphQlResponse> {
        let Self { plugins, terminal } = self;
        if let Some((plugin, remaining)) = plugins.split_first() {
            plugin.operation(
                context,
                Self {
                    plugins: remaining,
                    terminal,
                },
            )
        } else {
            terminal(context)
        }
    }
}

/// Runs the remaining field plugins or terminal exactly once.
pub struct FieldNext<'a, C> {
    plugins: &'a [Arc<dyn Plugin<C>>],
    terminal: FieldTerminal<'a, C>,
}

impl<'a, C: 'a + 'static> FieldNext<'a, C> {
    /// The middleware engine will create a field chain.
    #[doc(hidden)]
    pub fn new(
        plugins: &'a [Arc<dyn Plugin<C>>],
        terminal: impl FnOnce(FieldContext<'a, C>) -> BoxFuture<'a, FieldResult<ResolvedValue>>
        + Send
        + 'a,
    ) -> Self {
        Self {
            plugins,
            terminal: Box::new(terminal),
        }
    }

    /// Passes the context to the next plugin or terminal.
    pub fn run(self, context: FieldContext<'a, C>) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        let Self { plugins, terminal } = self;
        if let Some((plugin, remaining)) = plugins.split_first() {
            plugin.field(
                context,
                Self {
                    plugins: remaining,
                    terminal,
                },
            )
        } else {
            terminal(context)
        }
    }
}

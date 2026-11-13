//! Connect the ABI-compliant generated adapter to a common executor.

use std::sync::Arc;

use chigiri_core::FieldResult;
use chigiri_core::executor::{
    ArgumentValues, BoxFuture, FieldDispatch, ObjectScope, ResolvedObject, ResolvedValue,
    ResolverDispatch,
};

pub(crate) struct GeneratedDispatch<C>(pub(crate) Arc<dyn ResolverDispatch<C>>);

impl<C> FieldDispatch<C> for GeneratedDispatch<C> {
    fn object_scope(&self) -> Option<&ObjectScope> {
        None
    }

    fn resolve<'a>(
        &'a self,
        context: &'a C,
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.0
            .resolve(context, parent_type, field_name, parent, arguments)
    }
}

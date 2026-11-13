//! Exercises the large generated consumer from ADR-0007 as a regular crate.

#![forbid(unsafe_code)]

mod assembly;
mod enum_bindings;
pub use ::generated;
mod products_00;
mod products_01;
mod query;
mod resolver_macros;
mod types;

pub use assembly::assemble_resolvers;
pub use types::Types;

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    #[test]
    fn res_01_resolver_expressions_are_evaluated_once_in_source_order() {
        fn assert_send_sync<T: Send + Sync>() {}

        let evaluations = Arc::new(Mutex::new(Vec::new()));
        let _resolvers = super::assemble_resolvers(&evaluations);
        assert_send_sync::<super::generated::Resolvers<(), super::Types>>();

        assert_eq!(
            *evaluations
                .lock()
                .expect("evaluation records are available"),
            (0..71).collect::<Vec<_>>()
        );
    }
}

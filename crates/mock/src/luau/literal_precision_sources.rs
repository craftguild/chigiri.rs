//! Retain overflow literal precision at syntax-proven host source calls.

use full_moon::ast::{Call, Expression, Field, FunctionArgs, Index, Prefix, Suffix};

use super::literal_precision::Collector;
use super::literal_precision_syntax::identifier;

impl Collector<'_> {
    pub(super) fn source_input(&mut self, prefix: &Prefix, suffixes: &[&Suffix]) {
        let Prefix::Name(name) = prefix else {
            return;
        };
        let Some(name) = identifier(name) else {
            return;
        };
        if !self.aliases.declared_host_context(name) {
            return;
        }
        let [
            first,
            second,
            field,
            Suffix::Call(Call::AnonymousCall(arguments)),
        ] = suffixes
        else {
            return;
        };
        if dot(first) != Some("pubsub") || dot(second) != Some("subscribe") {
            return;
        }
        let Some(field) = dot(field) else {
            return;
        };
        let Some(root) = self.ir.graph().subscription() else {
            return;
        };
        if !self.ir.graph().types()[root.0]
            .fields
            .values()
            .any(|id| crate::events::payload::field_type_name(self.ir, *id) == field)
        {
            return;
        }
        let table = match arguments {
            FunctionArgs::Parentheses { arguments, .. } if arguments.len() == 1 => {
                match arguments.iter().next() {
                    Some(Expression::TableConstructor(table)) => table,
                    _ => return,
                }
            }
            FunctionArgs::TableConstructor(table) => table,
            _ => return,
        };
        for field in table.fields() {
            let Field::NameKey { key, value, .. } = field else {
                continue;
            };
            if identifier(key) == Some(crate::api::subscriptions::OVERFLOW) {
                self.contextual_expression(value, "ChigiriSubscriptionOverflow");
            }
        }
    }
}

fn dot(suffix: &Suffix) -> Option<&str> {
    match suffix {
        Suffix::Index(Index::Dot { name, .. }) => identifier(name),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::ir::MockIr;

    fn ir() -> MockIr {
        MockIr::parse(vec![chigiri::core::Source::new(
            "source.graphql",
            "schema{query:Read subscription:Signals} type Read{ping:Int} type Signals{count:Int!}",
        )])
        .unwrap()
    }

    #[test]
    fn source_overflow_precision_requires_an_unshadowed_generated_host_context() {
        let ir = ir();
        let input = "return host.pubsub.subscribe.Int({all_topics=true,overflow='drop_oldest'})";
        let source = format!(
            "local function select(host:ChigiriSignalsCountConsumerSetupContext) {input} end"
        );
        let narrowed = super::super::literal_precision::narrow(&source, &ir).0;
        assert_ne!(narrowed, source);
        assert!(narrowed.contains("ChigiriSubscriptionOverflow"));
        for (alias, body) in [
            ("any", input.to_owned()),
            ("ChigiriUserDefinedContext", input.to_owned()),
            (
                "ChigiriSignalsCountConsumerSetupContext",
                format!("local host = other {input}"),
            ),
            (
                "ChigiriSignalsCountConsumerSetupContext",
                format!("host = other {input}"),
            ),
        ] {
            let source = format!("local function select(host:{alias}) {body} end");
            assert_eq!(
                super::super::literal_precision::narrow(&source, &ir).0,
                source
            );
        }
        let source = format!(
            "type ChigiriSignalsCountConsumerSetupContext = any\nlocal function select(host:ChigiriSignalsCountConsumerSetupContext) {input} end"
        );
        assert_eq!(
            super::super::literal_precision::narrow(&source, &ir).0,
            source
        );
    }
}

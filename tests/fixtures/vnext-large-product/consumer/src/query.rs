//! Connects 60 query fields to a shared synthetic resolver.

use crate::resolver_macros::query_methods;
use crate::types::Types;

pub(crate) struct TrackingResolver;

impl ::generated::QueryResolver<(), Types, chigiri::FieldError> for TrackingResolver {
    async fn node<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<Option<::generated::Node<Types>>> {
        Ok(Some(::generated::Node::ProductObject000(
            crate::types::Product,
        )))
    }

    async fn search<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _args: ::generated::QuerySearchArgs,
    ) -> chigiri::FieldResult<Vec<::generated::SearchResult<Types>>> {
        Ok(vec![::generated::SearchResult::ProductObject001(
            crate::types::Product,
        )])
    }

    query_methods!(
        (object000, QueryObject000Args),
        (object001, QueryObject001Args),
        (object002, QueryObject002Args),
        (object003, QueryObject003Args),
        (object004, QueryObject004Args),
        (object005, QueryObject005Args),
        (object006, QueryObject006Args),
        (object007, QueryObject007Args),
        (object008, QueryObject008Args),
        (object009, QueryObject009Args),
        (object010, QueryObject010Args),
        (object011, QueryObject011Args),
        (object012, QueryObject012Args),
        (object013, QueryObject013Args),
        (object014, QueryObject014Args),
        (object015, QueryObject015Args),
        (object016, QueryObject016Args),
        (object017, QueryObject017Args),
        (object018, QueryObject018Args),
        (object019, QueryObject019Args),
        (object020, QueryObject020Args),
        (object021, QueryObject021Args),
        (object022, QueryObject022Args),
        (object023, QueryObject023Args),
        (object024, QueryObject024Args),
        (object025, QueryObject025Args),
        (object026, QueryObject026Args),
        (object027, QueryObject027Args),
        (object028, QueryObject028Args),
        (object029, QueryObject029Args),
        (object030, QueryObject030Args),
        (object031, QueryObject031Args),
        (object032, QueryObject032Args),
        (object033, QueryObject033Args),
        (object034, QueryObject034Args),
        (object035, QueryObject035Args),
        (object036, QueryObject036Args),
        (object037, QueryObject037Args),
        (object038, QueryObject038Args),
        (object039, QueryObject039Args),
        (object040, QueryObject040Args),
        (object041, QueryObject041Args),
        (object042, QueryObject042Args),
        (object043, QueryObject043Args),
        (object044, QueryObject044Args),
        (object045, QueryObject045Args),
        (object046, QueryObject046Args),
        (object047, QueryObject047Args),
        (object048, QueryObject048Args),
        (object049, QueryObject049Args),
        (object050, QueryObject050Args),
        (object051, QueryObject051Args),
        (object052, QueryObject052Args),
        (object053, QueryObject053Args),
        (object054, QueryObject054Args),
        (object055, QueryObject055Args),
        (object056, QueryObject056Args),
        (object057, QueryObject057Args),
        (object058, QueryObject058Args),
        (object059, QueryObject059Args),
    );
}

impl ::generated::MutationResolver<(), Types, chigiri::FieldError> for TrackingResolver {
    async fn update_object<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _args: ::generated::MutationUpdateObjectArgs<Types>,
    ) -> chigiri::FieldResult<crate::types::Product> {
        Ok(crate::types::Product)
    }
}

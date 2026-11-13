#![deny(warnings)]

pub use ::generated;

use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;

pub struct RequestContext {
    pub request_label: String,
}

pub struct Types;

pub struct SubscriptionRoot {
    pub tick: i32,
}

pub struct Presence {
    pub team_id: chigiri::Id,
    pub online: i32,
}

impl generated::SchemaTypes for Types {
    type Subscription = SubscriptionRoot;
    type Presence = Presence;
}

pub struct QueryFields;

impl generated::QueryResolver<RequestContext, Types, chigiri::FieldError> for QueryFields {
    async fn health<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, RequestContext>,
    ) -> chigiri::FieldResult<bool> {
        Ok(true)
    }

    async fn subscription<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, RequestContext>,
    ) -> chigiri::FieldResult<SubscriptionRoot> {
        Ok(SubscriptionRoot { tick: 11 })
    }
}

pub struct PresenceFields;

impl generated::PresenceResolver<RequestContext, Types, chigiri::FieldError> for PresenceFields {
    async fn team_id<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, RequestContext>,
        parent: &'a Presence,
    ) -> chigiri::FieldResult<chigiri::Id> {
        Ok(parent.team_id.clone())
    }

    async fn online<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, RequestContext>,
        parent: &'a Presence,
    ) -> chigiri::FieldResult<i32> {
        Ok(parent.online)
    }
}

pub struct OwnedEvents<Event>(PhantomData<Event>);

impl<Event> OwnedEvents<Event> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<Event> Stream for OwnedEvents<Event> {
    type Item = chigiri::FieldResult<Event>;

    fn poll_next(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

pub struct NonSendEvents<Event> {
    marker: std::rc::Rc<()>,
    event: PhantomData<Event>,
}

impl<Event> NonSendEvents<Event> {
    pub fn new() -> Self {
        Self {
            marker: std::rc::Rc::new(()),
            event: PhantomData,
        }
    }
}

impl<Event> Stream for NonSendEvents<Event> {
    type Item = chigiri::FieldResult<Event>;

    fn poll_next(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let _ = &self.marker;
        Poll::Ready(None)
    }
}

pub struct BorrowedEvents<'a, Event> {
    pub borrowed: &'a chigiri::Id,
    pub marker: PhantomData<Event>,
}

impl<Event> Stream for BorrowedEvents<'_, Event> {
    type Item = chigiri::FieldResult<Event>;

    fn poll_next(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let _ = self.borrowed;
        Poll::Ready(None)
    }
}

pub struct SubscriptionFields {
    pub resolver_label: String,
}

impl generated::SubscriptionResolver<RequestContext, Types, chigiri::FieldError>
    for SubscriptionFields
{
    type PresenceEvent = i32;
    type TickEvent = i32;

    async fn subscribe_presence<'a>(
        &'a self,
        ctx: chigiri::FieldContext<'a, RequestContext>,
        args: &'a generated::SubscriptionPresenceArgs,
    ) -> chigiri::FieldResult<chigiri::SubscriptionSource<Self::PresenceEvent, chigiri::FieldError>>
    {
        let _borrowed_until_setup_completes = (
            &self.resolver_label,
            &ctx.context().request_label,
            &args.team_id,
        );
        Ok(chigiri::SubscriptionSource::new(OwnedEvents::<
            Self::PresenceEvent,
        >::new()))
    }

    async fn resolve_presence_event<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, RequestContext>,
        args: &'a generated::SubscriptionPresenceArgs,
        _event: &'a Self::PresenceEvent,
    ) -> chigiri::FieldResult<Presence> {
        Ok(Presence {
            team_id: args.team_id.clone(),
            online: 1,
        })
    }

    async fn presence<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, RequestContext>,
        _parent: &'a SubscriptionRoot,
        args: generated::SubscriptionPresenceArgs,
    ) -> chigiri::FieldResult<Presence> {
        Ok(Presence {
            team_id: args.team_id,
            online: 2,
        })
    }

    async fn subscribe_tick<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, RequestContext>,
    ) -> chigiri::FieldResult<chigiri::SubscriptionSource<Self::TickEvent, chigiri::FieldError>>
    {
        Ok(chigiri::SubscriptionSource::new(OwnedEvents::<
            Self::TickEvent,
        >::new()))
    }

    async fn resolve_tick_event<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, RequestContext>,
        event: &'a Self::TickEvent,
    ) -> chigiri::FieldResult<i32> {
        Ok(*event)
    }

    async fn tick<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, RequestContext>,
        parent: &'a SubscriptionRoot,
    ) -> chigiri::FieldResult<i32> {
        Ok(parent.tick)
    }
}

pub fn resolvers() -> generated::Resolvers<RequestContext, Types> {
    generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(QueryFields)
        .with_subscription(SubscriptionFields {
            resolver_label: "fixture".to_owned(),
        })
        .with_presence(PresenceFields)
        .build()
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    use chigiri::__private::{
        ArgumentValues, ResolvedObject, ResolvedValue, ResolverDispatch,
        SubscriptionResolverDispatch,
    };

    use super::{RequestContext, SubscriptionRoot, resolvers};

    fn block_on<F: Future>(future: F) -> F::Output {
        let mut context = Context::from_waker(Waker::noop());
        let mut future = std::pin::pin!(future);
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn one_group_exposes_subscription_and_nested_ordinary_dispatch() {
        let resolvers = resolvers();
        assert_eq!(resolvers.subscription_root(), Some("Subscription"));
        let context = RequestContext {
            request_label: "request".to_owned(),
        };
        let parent = ResolvedObject::new("Subscription", SubscriptionRoot { tick: 17 });
        let value = block_on(resolvers.resolve(
            &context,
            "Subscription",
            "tick",
            Some(&parent),
            ArgumentValues::new(vec![]),
        ))
        .unwrap();
        assert!(matches!(
            value,
            ResolvedValue::Leaf(chigiri::Value::Int(17))
        ));
        let missing = block_on(resolvers.resolve(
            &context,
            "Subscription",
            "tick",
            None,
            ArgumentValues::new(vec![]),
        ));
        assert!(missing.is_err());
    }
}

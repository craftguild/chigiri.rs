#![deny(warnings)]

pub use ::generated;

use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;

pub struct Types;

pub struct Presence {
    pub id: chigiri::Id,
}

pub struct Alert {
    pub id: chigiri::Id,
}

impl generated::SchemaTypes for Types {
    type Presence = Presence;
    type Alert = Alert;
}

#[derive(Debug)]
pub struct AppError;

impl std::fmt::Display for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("application error")
    }
}

impl std::error::Error for AppError {}

#[derive(Debug)]
pub struct OtherError;

impl std::fmt::Display for OtherError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("other application error")
    }
}

impl std::error::Error for OtherError {}

pub struct OwnedEvents<Event, Error>(PhantomData<(Event, Error)>);

impl<Event, Error> OwnedEvents<Event, Error> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<Event, Error> Stream for OwnedEvents<Event, Error> {
    type Item = Result<Event, Error>;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

pub struct QueryFields;

impl generated::QueryResolver<(), Types, AppError> for QueryFields {
    async fn health<'a>(&'a self, _: chigiri::FieldContext<'a, ()>) -> Result<bool, AppError> {
        Ok(true)
    }
}

pub struct PresenceFields;

impl generated::PresenceResolver<(), Types, AppError> for PresenceFields {
    async fn id<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        parent: &'a Presence,
    ) -> Result<chigiri::Id, AppError> {
        Ok(parent.id.clone())
    }
}

pub struct AlertFields;

impl generated::AlertResolver<(), Types, AppError> for AlertFields {
    async fn id<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        parent: &'a Alert,
    ) -> Result<chigiri::Id, AppError> {
        Ok(parent.id.clone())
    }
}

fn presence() -> Presence {
    Presence { id: "p-1".into() }
}

fn alert() -> Alert {
    Alert { id: "a-1".into() }
}

pub struct SubscriptionFields;

// BEGIN SUBSCRIPTION GROUP
impl generated::SubscriptionResolver<(), Types, AppError> for SubscriptionFields {
    type MaybePresencesEvent = i32;
    type NodeEvent = i32;
    type ResultEvent = i32;

    async fn subscribe_maybe_presences<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> Result<chigiri::SubscriptionSource<Self::MaybePresencesEvent, AppError>, AppError> {
        Ok(chigiri::SubscriptionSource::new(OwnedEvents::<
            Self::MaybePresencesEvent,
            AppError,
        >::new()))
    }

    async fn resolve_maybe_presences_event<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        _: &'a Self::MaybePresencesEvent,
    ) -> Result<Option<Vec<Option<Presence>>>, AppError> {
        Ok(Some(vec![Some(presence())]))
    }

    async fn subscribe_node<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> Result<chigiri::SubscriptionSource<Self::NodeEvent, AppError>, AppError> {
        Ok(chigiri::SubscriptionSource::new(OwnedEvents::<
            Self::NodeEvent,
            AppError,
        >::new()))
    }

    async fn resolve_node_event<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        _: &'a Self::NodeEvent,
    ) -> Result<generated::EventNode<Types>, AppError> {
        Ok(generated::EventNode::Presence(presence()))
    }

    async fn subscribe_result<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> Result<chigiri::SubscriptionSource<Self::ResultEvent, AppError>, AppError> {
        Ok(chigiri::SubscriptionSource::new(OwnedEvents::<
            Self::ResultEvent,
            AppError,
        >::new()))
    }

    async fn resolve_result_event<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        _: &'a Self::ResultEvent,
    ) -> Result<generated::EventResult<Types>, AppError> {
        Ok(generated::EventResult::Alert(alert()))
    }
}
// END SUBSCRIPTION GROUP

pub fn resolvers() -> generated::Resolvers<(), Types> {
    generated::Resolvers::builder(|_: AppError| chigiri::FieldError::public("application error"))
        .with_presence(PresenceFields)
        .with_alert(AlertFields)
        .with_query(QueryFields)
        .with_subscription(SubscriptionFields)
        .build()
}

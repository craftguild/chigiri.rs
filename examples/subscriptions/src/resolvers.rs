//! Source setup, event-to-root mapping and selected fields are distinct roles.

use chigiri::{FieldContext, SubscriptionSource};

use crate::domain::{AppError, Reading, State};

pub(crate) struct Types;

impl generated::SchemaTypes for Types {
    type Reading = Reading;
}

#[derive(Clone)]
pub(crate) struct Fields(pub(crate) State);

impl generated::QueryResolver<(), Types, AppError> for Fields {
    async fn current<'a>(&'a self, _: FieldContext<'a, ()>) -> Result<Reading, AppError> {
        Ok(self.0.current())
    }
}

impl generated::SubscriptionResolver<(), Types, AppError> for Fields {
    type ReadingsEvent = Reading;

    async fn subscribe_readings<'a>(
        &'a self,
        _: FieldContext<'a, ()>,
    ) -> Result<SubscriptionSource<Reading, AppError>, AppError> {
        // The returned source owns its interval and Arc; it borrows no setup
        // request, context, argument or temporary producer task.
        Ok(SubscriptionSource::new(self.0.source()))
    }

    async fn resolve_readings_event<'a>(
        &'a self,
        _: FieldContext<'a, ()>,
        event: &'a Reading,
    ) -> Result<Reading, AppError> {
        // This copies the small root and its Arc, not the complete message.
        Ok(event.clone())
    }
}

impl generated::ReadingResolver<(), Types, AppError> for Fields {
    async fn number<'a>(
        &'a self,
        _: FieldContext<'a, ()>,
        parent: &'a Reading,
    ) -> Result<i32, AppError> {
        Ok(parent.number)
    }

    async fn message<'a>(
        &'a self,
        _: FieldContext<'a, ()>,
        parent: &'a Reading,
    ) -> Result<String, AppError> {
        // Only a selected field materializes its output string for encoding.
        Ok(parent.message.to_string())
    }
}

pub(crate) fn map_error(_: AppError) -> chigiri::FieldError {
    chigiri::FieldError::public("Reading source unavailable").code("READING_UNAVAILABLE")
}

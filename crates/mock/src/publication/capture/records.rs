//! Recapture genuine record slots without consulting or rebinding a current row.

use super::{
    CaptureWriter, CapturedField, CapturedOrigin, DIGEST_BYTES, PublicationError, record_digest,
};
use crate::ir::{ModelId, SlotId, TypeId};

impl CaptureWriter<'_> {
    pub(super) fn record_origin<'a>(
        &mut self,
        model: ModelId,
        ty: TypeId,
        values: impl ExactSizeIterator<Item = (SlotId, &'a chigiri::Value)>,
        identity: Option<&str>,
        type_name: &str,
    ) -> Result<(CapturedOrigin, Vec<CapturedField>), PublicationError> {
        let definition = self
            .ir
            .models()
            .get(model.0)
            .ok_or(PublicationError::InvalidPayload)?;
        if definition.ty != ty {
            return Err(PublicationError::InvalidPayload);
        }
        let fields = self.record_fields(values)?;
        let model_name = self.text(&self.ir.graph().types()[definition.ty.0].name)?;
        let public_identity = identity.map(|value| self.text(value)).transpose()?;
        self.meter.add(DIGEST_BYTES)?;
        let digest = record_digest(
            self.ir.graph().source().digest(),
            type_name,
            &model_name,
            public_identity.as_deref(),
            &fields,
        );
        Ok((
            CapturedOrigin::Record {
                model: model_name,
                public_identity,
                digest,
            },
            fields,
        ))
    }
}

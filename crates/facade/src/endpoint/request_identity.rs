//! Worker-local initialization and first use of the request-identity factory.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chigiri_observability::RandomRequestIdFactory;
use chigiri_runtime::{RequestId, RequestIdFactory, RequestIdFactoryError};

use crate::build_support::EndpointBuildError;
use crate::draft::RequestIdPlan;

pub(super) fn initialize(plan: &mut RequestIdPlan) -> Result<(), EndpointBuildError> {
    initialize_with(plan, || {
        Ok(
            Arc::new(RandomRequestIdFactory::new().map_err(|_| EndpointBuildError::identity())?)
                as Arc<dyn RequestIdFactory>,
        )
    })
}

fn initialize_with(
    plan: &mut RequestIdPlan,
    automatic: impl FnOnce() -> Result<Arc<dyn RequestIdFactory>, EndpointBuildError>,
) -> Result<(), EndpointBuildError> {
    let factory = match plan {
        RequestIdPlan::Automatic => automatic()?,
        RequestIdPlan::Configured(factory) => Arc::clone(factory),
    };
    let first = factory
        .create()
        .map_err(|_| EndpointBuildError::identity())?;
    *plan = RequestIdPlan::Configured(Arc::new(PrimedRequestIds {
        first,
        first_pending: AtomicBool::new(true),
        creator_process_id: std::process::id(),
        factory,
    }));
    Ok(())
}

impl RequestIdPlan {
    pub(super) fn configured(&self) -> Option<&Arc<dyn RequestIdFactory>> {
        match self {
            Self::Automatic => None,
            Self::Configured(factory) => Some(factory),
        }
    }
}

struct PrimedRequestIds {
    first: RequestId,
    first_pending: AtomicBool,
    creator_process_id: u32,
    factory: Arc<dyn RequestIdFactory>,
}

impl PrimedRequestIds {
    fn create_for_process(&self, process_id: u32) -> Result<RequestId, RequestIdFactoryError> {
        if process_id != self.creator_process_id {
            return Err(RequestIdFactoryError::new(ProcessChanged));
        }
        if self.first_pending.load(Ordering::Acquire)
            && self
                .first_pending
                .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            return Ok(self.first.clone());
        }
        self.factory.create()
    }
}

impl RequestIdFactory for PrimedRequestIds {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        self.create_for_process(std::process::id())
    }
}

#[derive(Debug)]
struct ProcessChanged;

impl std::fmt::Display for ProcessChanged {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("request ID factory cannot cross a process boundary")
    }
}

impl std::error::Error for ProcessChanged {}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chigiri_runtime::{RequestId, RequestIdFactory, RequestIdFactoryError};

    use super::{PrimedRequestIds, initialize_with};
    use crate::EndpointBuildError;
    use crate::draft::RequestIdPlan;

    struct CountingFactory(AtomicUsize);

    impl RequestIdFactory for CountingFactory {
        fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
            let next = self.0.fetch_add(1, Ordering::SeqCst);
            RequestId::new(format!("request-{next:016x}")).map_err(RequestIdFactoryError::new)
        }
    }

    #[test]
    fn primed_request_id_is_restricted_to_the_creator_process() {
        let factory = Arc::new(CountingFactory(AtomicUsize::new(0)));
        let primed = PrimedRequestIds {
            first: RequestId::new("request-primed").unwrap(),
            first_pending: std::sync::atomic::AtomicBool::new(true),
            creator_process_id: 100,
            factory: factory.clone(),
        };
        assert!(primed.create_for_process(101).is_err());
        assert_eq!(factory.0.load(Ordering::SeqCst), 0);
        assert_eq!(
            primed.create_for_process(100).unwrap().as_str(),
            "request-primed"
        );
    }

    #[test]
    fn automatic_factory_initializes_and_probes_once_at_build_time() {
        let factory = Arc::new(CountingFactory(AtomicUsize::new(0)));
        let constructions = AtomicUsize::new(0);
        let mut plan = RequestIdPlan::Automatic;
        initialize_with(&mut plan, || {
            constructions.fetch_add(1, Ordering::SeqCst);
            Ok(factory.clone())
        })
        .unwrap();
        assert_eq!(constructions.load(Ordering::SeqCst), 1);
        assert_eq!(factory.0.load(Ordering::SeqCst), 1);
        let request_id = plan.configured().unwrap().create().unwrap();
        assert_eq!(request_id.as_str(), "request-0000000000000000");
        assert_eq!(factory.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn automatic_factory_build_failure_returns_identity_initialization_error() {
        let mut plan = RequestIdPlan::Automatic;
        let error = initialize_with(&mut plan, || Err(EndpointBuildError::identity())).unwrap_err();
        assert!(matches!(error, EndpointBuildError::IdentityInitialization));
        assert!(matches!(plan, RequestIdPlan::Automatic));
    }
}

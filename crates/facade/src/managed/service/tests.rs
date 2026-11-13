//! SDK activation/failure and last-owner tests, independent of native adapters.

use std::time::Duration;

use super::*;
use crate::Endpoint;
use crate::subscription::request::test_support::schema;

fn endpoint() -> Endpoint<()> {
    Endpoint::builder(schema())
        .without_context()
        .build()
        .unwrap()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

fn wait_for_join(runtime: &tokio::runtime::Runtime, endpoint: &Endpoint<()>) {
    let control = endpoint.managed_control();
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(2), async {
            while crate::subscription::worker::managed_supervisor_retained_for_test(&control) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("owned SDK supervisor must physically join");
    });
}

#[test]
fn construction_without_runtime_is_fallible_and_does_not_bind_the_endpoint() {
    let endpoint = endpoint();
    assert!(matches!(
        endpoint.__managed_service(),
        Err(ManagedServiceError::MissingRuntime)
    ));
    let runtime = runtime();
    let _entered = runtime.enter();
    let service = endpoint.__managed_service().unwrap();
    drop(service);
    wait_for_join(&runtime, &endpoint);
}

#[test]
fn missing_timer_is_a_construction_error_without_a_panicking_api() {
    let endpoint = endpoint();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    {
        let _entered = runtime.enter();
        assert!(matches!(
            endpoint.__managed_service(),
            Err(ManagedServiceError::Startup)
        ));
    }
    assert_eq!(endpoint.shutdown_outcome(), None);
    let timed = self::runtime();
    let _entered = timed.enter();
    drop(endpoint.__managed_service().unwrap());
    wait_for_join(&timed, &endpoint);
}

#[test]
fn activation_resource_failure_selects_infrastructure_without_returning_a_service() {
    let runtime = runtime();
    let _entered = runtime.enter();
    let endpoint = endpoint();
    let binding = Binding::new();
    let result = binding.activate(usize::MAX, || {
        let guard = endpoint.__register_subscription_runtime().unwrap();
        let (registration, driver) = endpoint.__http_streaming_supervision().unwrap();
        Ok((registration, driver, guard))
    });
    assert!(matches!(result, Err(ManagedServiceError::Capacity)));
    assert_eq!(
        binding.control.outcome(),
        Some(crate::ShutdownOutcome::InfrastructureFailure)
    );
    assert!(binding.control.admit().is_err());
    assert!(!crate::subscription::worker::managed_supervisor_retained_for_test(&binding.control));
}

#[test]
fn constructors_and_clones_share_one_owner_and_only_last_service_drop_cancels() {
    let runtime = runtime();
    let _entered = runtime.enter();
    let endpoint = endpoint();
    let service = endpoint.__managed_service().unwrap();
    let second = endpoint.clone().__managed_service().unwrap();
    let cloned = second.clone();
    assert!(Arc::ptr_eq(&service.life, &second.life));
    let scope = service.request().unwrap();
    let weak = Arc::downgrade(&service.life);
    drop(service);
    drop(second);
    assert_eq!(endpoint.shutdown_outcome(), None);
    assert!(weak.upgrade().is_some());
    drop(cloned);
    assert!(
        weak.upgrade().is_none(),
        "request work must not retain service ownership"
    );
    assert_eq!(
        endpoint.shutdown_outcome(),
        Some(crate::ShutdownOutcome::CancellationRequested)
    );
    assert!(!endpoint.managed_control().requests_idle());
    drop(scope);
    assert!(endpoint.managed_control().requests_idle());
    assert!(matches!(
        endpoint.__managed_service(),
        Err(ManagedServiceError::Closed)
    ));
    wait_for_join(&runtime, &endpoint);
}

#[test]
fn an_endpoint_binding_cannot_move_to_another_runtime() {
    let first = runtime();
    let second = runtime();
    let endpoint = endpoint();
    let service = {
        let _entered = first.enter();
        endpoint.__managed_service().unwrap()
    };
    {
        let _entered = second.enter();
        assert!(matches!(
            endpoint.__managed_service(),
            Err(ManagedServiceError::RuntimeMismatch)
        ));
        assert!(matches!(
            service.request(),
            Err(ManagedServiceError::RuntimeMismatch)
        ));
        let separate = self::endpoint();
        let independent = separate.__managed_service().unwrap();
        drop(independent);
        assert_eq!(endpoint.shutdown_outcome(), None);
    }
    drop(service);
    wait_for_join(&first, &endpoint);
}

#[test]
fn aborting_the_actual_supervisor_closes_admission_and_records_failure() {
    let runtime = runtime();
    runtime.block_on(async {
        let endpoint = endpoint();
        let service = endpoint.__managed_service().unwrap();
        let control = endpoint.managed_control();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !crate::subscription::worker::abort_managed_for_test(&control) {
                tokio::task::yield_now().await;
            }
            while endpoint.shutdown_outcome().is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(crate::ShutdownOutcome::InfrastructureFailure)
        );
        assert!(matches!(
            service.request(),
            Err(ManagedServiceError::Closed)
        ));
        assert!(matches!(
            endpoint.__managed_service(),
            Err(ManagedServiceError::Closed)
        ));
        drop(service);
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(crate::ShutdownOutcome::InfrastructureFailure)
        );
        tokio::time::timeout(Duration::from_secs(2), async {
            while crate::subscription::worker::managed_supervisor_retained_for_test(&control) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    });
}

#[test]
fn concurrent_construction_and_last_release_never_publish_an_unsupervised_owner() {
    let runtime = runtime();
    for _ in 0..8 {
        let endpoint = endpoint();
        let original = {
            let _entered = runtime.enter();
            endpoint.__managed_service().unwrap()
        };
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let dropping = Arc::clone(&barrier);
        let release = std::thread::spawn(move || {
            dropping.wait();
            drop(original);
        });
        let constructing = Arc::clone(&barrier);
        let candidate = endpoint.clone();
        let handle = runtime.handle().clone();
        let activation = std::thread::spawn(move || {
            let _entered = handle.enter();
            constructing.wait();
            candidate.__managed_service()
        });
        release.join().unwrap();
        match activation.join().unwrap() {
            Ok(service) => {
                assert_eq!(endpoint.shutdown_outcome(), None);
                {
                    let _entered = runtime.enter();
                    drop(service.request().expect("live shared service admission"));
                }
                drop(service);
            }
            Err(
                ManagedServiceError::Closed
                | ManagedServiceError::Infrastructure
                | ManagedServiceError::Startup,
            ) => {}
            Err(other) => panic!("unexpected activation race classification: {other:?}"),
        }
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(crate::ShutdownOutcome::CancellationRequested)
        );
        wait_for_join(&runtime, &endpoint);
    }
}

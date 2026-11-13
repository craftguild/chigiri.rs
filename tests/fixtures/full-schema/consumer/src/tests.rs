//! Exercises variables, input arguments, and projection output through the public contract.

use std::future::Future;
use std::task::{Context, Poll, Waker};

use contract::{MeasurementResolver as _, SchemaTypes as _};

use crate::{MeasurementFields, SampleMeasurement, Types};

fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("synchronous projection unexpectedly remained pending"),
    }
}

#[test]
fn full_sdl_arguments_cross_the_generated_adapter_with_domain_type_safety() {
    let input = contract::QueryMeasurementsArgs::<Types> {
        sequence: chigiri::Input::Undefined,
        transport: chigiri::Input::Value(
            Types::transport_kind_from_graphql(contract::TransportKind::Value169).unwrap(),
        ),
        format: chigiri::Input::Value(
            Types::format_kind_from_graphql(contract::FormatKind::Value180).unwrap(),
        ),
        first: Some(50),
        after: chigiri::Input::Undefined,
    };
    assert_eq!(
        input.transport,
        chigiri::Input::Value(domain::ExternalTransport::Value169)
    );
    assert_eq!(
        input.format,
        chigiri::Input::Value(domain::FormatKind::Value180)
    );
    for value in [
        domain::FormatKind::Value176,
        domain::FormatKind::Value180,
        domain::FormatKind::Value179,
    ] {
        let wire = Types::format_kind_to_graphql(&value).unwrap();
        assert_eq!(Types::format_kind_from_graphql(wire).unwrap(), value);
    }
    for value in [
        domain::ExternalTransport::Value168,
        domain::ExternalTransport::Value169,
    ] {
        let wire = Types::transport_kind_to_graphql(&value).unwrap();
        assert_eq!(Types::transport_kind_from_graphql(wire).unwrap(), value);
    }
}

#[test]
fn converts_external_unit_domain_types_from_full_projection_back_to_wire() {
    let resolver = MeasurementFields;
    for unit in [
        domain::UnitKind::Value173,
        domain::UnitKind::Value075,
        domain::UnitKind::Value174,
    ] {
        let measurement = SampleMeasurement {
            value: "1200".to_owned(),
            unit,
            precision: 2,
        };
        let context = chigiri::FieldContext::new(&(), "Measurement", "unit");
        let output: domain::UnitKind = ready(resolver.unit(context, &measurement)).unwrap();
        assert_eq!(output, unit);
        let wire = Types::unit_kind_to_graphql(&output).unwrap();
        assert_eq!(Types::unit_kind_from_graphql(wire).unwrap(), unit);
        assert_eq!(
            ready(resolver.value(context, &measurement)).unwrap().0,
            "1200"
        );
        assert_eq!(ready(resolver.precision(context, &measurement)).unwrap(), 2);
    }
    contract::generator().validate().unwrap();
    assert_eq!(chigiri::GENERATED_ABI_VERSION, 8);
}

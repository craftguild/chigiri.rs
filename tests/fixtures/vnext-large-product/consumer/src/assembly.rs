//! Exercises the type-state chain and resolver assembly for 70 objects.

use std::sync::{Arc, Mutex};

use crate::query::TrackingResolver;
use crate::types::Types;

/// Evaluates each resolver source once before creating the final assembly.
pub fn assemble_resolvers(
    evaluations: &Arc<Mutex<Vec<usize>>>,
) -> ::generated::Resolvers<(), Types> {
    let query = evaluated(evaluations, 0);
    let mutation = evaluated(evaluations, 1);
    let product000 = evaluated(evaluations, 2);
    ::generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(query)
        .with_mutation(mutation)
        .with_product_object000(product000)
        .with_product_object001(evaluated(evaluations, 3))
        .with_product_object002(evaluated(evaluations, 4))
        .with_product_object003(evaluated(evaluations, 5))
        .with_product_object004(evaluated(evaluations, 6))
        .with_product_object005(evaluated(evaluations, 7))
        .with_product_object006(evaluated(evaluations, 8))
        .with_product_object007(evaluated(evaluations, 9))
        .with_product_object008(evaluated(evaluations, 10))
        .with_product_object009(evaluated(evaluations, 11))
        .with_product_object010(evaluated(evaluations, 12))
        .with_product_object011(evaluated(evaluations, 13))
        .with_product_object012(evaluated(evaluations, 14))
        .with_product_object013(evaluated(evaluations, 15))
        .with_product_object014(evaluated(evaluations, 16))
        .with_product_object015(evaluated(evaluations, 17))
        .with_product_object016(evaluated(evaluations, 18))
        .with_product_object017(evaluated(evaluations, 19))
        .with_product_object018(evaluated(evaluations, 20))
        .with_product_object019(evaluated(evaluations, 21))
        .with_product_object020(evaluated(evaluations, 22))
        .with_product_object021(evaluated(evaluations, 23))
        .with_product_object022(evaluated(evaluations, 24))
        .with_product_object023(evaluated(evaluations, 25))
        .with_product_object024(evaluated(evaluations, 26))
        .with_product_object025(evaluated(evaluations, 27))
        .with_product_object026(evaluated(evaluations, 28))
        .with_product_object027(evaluated(evaluations, 29))
        .with_product_object028(evaluated(evaluations, 30))
        .with_product_object029(evaluated(evaluations, 31))
        .with_product_object030(evaluated(evaluations, 32))
        .with_product_object031(evaluated(evaluations, 33))
        .with_product_object032(evaluated(evaluations, 34))
        .with_product_object033(evaluated(evaluations, 35))
        .with_product_object034(evaluated(evaluations, 36))
        .with_product_object035(evaluated(evaluations, 37))
        .with_product_object036(evaluated(evaluations, 38))
        .with_product_object037(evaluated(evaluations, 39))
        .with_product_object038(evaluated(evaluations, 40))
        .with_product_object039(evaluated(evaluations, 41))
        .with_product_object040(evaluated(evaluations, 42))
        .with_product_object041(evaluated(evaluations, 43))
        .with_product_object042(evaluated(evaluations, 44))
        .with_product_object043(evaluated(evaluations, 45))
        .with_product_object044(evaluated(evaluations, 46))
        .with_product_object045(evaluated(evaluations, 47))
        .with_product_object046(evaluated(evaluations, 48))
        .with_product_object047(evaluated(evaluations, 49))
        .with_product_object048(evaluated(evaluations, 50))
        .with_product_object049(evaluated(evaluations, 51))
        .with_product_object050(evaluated(evaluations, 52))
        .with_product_object051(evaluated(evaluations, 53))
        .with_product_object052(evaluated(evaluations, 54))
        .with_product_object053(evaluated(evaluations, 55))
        .with_product_object054(evaluated(evaluations, 56))
        .with_product_object055(evaluated(evaluations, 57))
        .with_product_object056(evaluated(evaluations, 58))
        .with_product_object057(evaluated(evaluations, 59))
        .with_product_object058(evaluated(evaluations, 60))
        .with_product_object059(evaluated(evaluations, 61))
        .with_product_object060(evaluated(evaluations, 62))
        .with_product_object061(evaluated(evaluations, 63))
        .with_product_object062(evaluated(evaluations, 64))
        .with_product_object063(evaluated(evaluations, 65))
        .with_product_object064(evaluated(evaluations, 66))
        .with_product_object065(evaluated(evaluations, 67))
        .with_product_object066(evaluated(evaluations, 68))
        .with_product_object067(evaluated(evaluations, 69))
        .with_product_object068(evaluated(evaluations, 70))
        .build()
}

fn evaluated(evaluations: &Arc<Mutex<Vec<usize>>>, index: usize) -> TrackingResolver {
    evaluations
        .lock()
        .expect("evaluation records must be writable")
        .push(index);
    TrackingResolver
}

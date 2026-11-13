//! Decode typed arguments for host handlers and encode results against the declared return type.

use std::sync::Arc;

use chigiri::core::{Object, Value};

use super::state::Host;
use crate::api::{Method, MethodSpec, TypeExpr};
use crate::codec::{Codec, VmValue};
use crate::ir::ModelId;
use crate::store::Record;
use crate::value::ValueError;

enum ResultValue {
    Records(Vec<Arc<Record>>),
    Record(Arc<Record>),
    Number(usize),
}

pub(super) fn invoke(
    host: &mut Host,
    codec: &mut Codec,
    model: ModelId,
    spec: &MethodSpec,
    args: &[VmValue],
) -> Result<VmValue, ValueError> {
    let ir = host.ir.clone();
    let mut decoded = Vec::new();
    for (parameter, value) in spec.arguments.iter().zip(args) {
        let value = match parameter.ty {
            TypeExpr::FindOptions => codec.decode_query(&ir, model, false, value),
            TypeExpr::CountOptions => codec.decode_query(&ir, model, true, value),
            TypeExpr::TargetOptions => {
                let name = decoded
                    .get(1)
                    .ok_or(ValueError("relation argument required"))?;
                let (_, target) = ir
                    .relation_target(model, string(name)?)
                    .ok_or(ValueError("relation field required"))?;
                codec.decode_query(&ir, target, false, value)
            }
            TypeExpr::Relation => codec.decode_name(&ir, value),
            TypeExpr::CreateInput | TypeExpr::UpdateInput => codec.decode_record(&ir, model, value),
            _ => Err(ValueError("invalid argument contract")),
        }?;
        decoded.push(value);
    }
    let args = decoded;
    let arg = |index| args.get(index).ok_or(ValueError("missing host argument"));
    let result = match spec.handler {
        Method::Count => ResultValue::Number(host.count(model, object(arg(0)?)?)?),
        Method::Find => ResultValue::Records(host.find(model, object(arg(0)?)?)?),
        Method::Create => ResultValue::Record(host.write()?.create(model, object(arg(0)?)?)?),
        Method::Update => ResultValue::Records(host.write()?.update(
            model,
            object(arg(0)?)?,
            object(arg(1)?)?,
        )?),
        Method::Delete => ResultValue::Number(host.write()?.delete(model, object(arg(0)?)?)?),
        Method::Connect | Method::Disconnect => {
            ResultValue::Records(host.write()?.relate_selected(
                model,
                object(arg(0)?)?,
                string(arg(1)?)?,
                object(arg(2)?)?,
                spec.handler == Method::Connect,
            )?)
        }
    };
    match (spec.returns, result) {
        (TypeExpr::Number, ResultValue::Number(value)) => codec.encode(&Value::Int(value as i128)),
        (TypeExpr::Records, ResultValue::Records(rows)) => codec.encode_records(&ir, &rows),
        (TypeExpr::Record, ResultValue::Record(row)) => codec.encode_record(&ir, &row),
        _ => Err(ValueError("invalid return contract")),
    }
}

fn object(value: &Value) -> Result<&Object, ValueError> {
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(ValueError("host object required")),
    }
}

fn string(value: &Value) -> Result<&str, ValueError> {
    match value {
        Value::String(value) => Ok(value),
        _ => Err(ValueError("host relation name string required")),
    }
}

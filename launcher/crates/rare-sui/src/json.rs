//! protobuf `Value` (the fullnode's rendered Move JSON and Display output) to `serde_json::Value`.

use prost_types::value::Kind;
use serde_json::{Map, Number, Value};

pub fn from_proto(v: &prost_types::Value) -> Value {
    match &v.kind {
        None | Some(Kind::NullValue(_)) => Value::Null,
        Some(Kind::BoolValue(b)) => Value::Bool(*b),
        Some(Kind::NumberValue(n)) => {
            if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
                Value::Number(Number::from(*n as i64))
            } else {
                Number::from_f64(*n).map(Value::Number).unwrap_or(Value::Null)
            }
        }
        Some(Kind::StringValue(s)) => Value::String(s.clone()),
        Some(Kind::ListValue(l)) => Value::Array(l.values.iter().map(from_proto).collect()),
        Some(Kind::StructValue(s)) => Value::Object(s.fields.iter().map(|(k, v)| (k.clone(), from_proto(v))).collect::<Map<_, _>>()),
    }
}

/// A JSON field as a string id: either `"0x.."` or `{ "id": "0x.." }` (`idText` in licence.ts).
pub fn id_text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => o.get("id").and_then(|x| x.as_str()).map(str::to_string),
        _ => None,
    }
}

/// `Number(x ?? 0)`: numbers and decimal strings (u64s come as strings), else 0.
pub fn num(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(f64::NAN),
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        _ => 0.0,
    }
}

/// A finite number, else None (`num` in wallet/data/prices.ts).
pub fn finite(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|n| n.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::{ListValue, Struct};

    #[test]
    fn converts_nested_values() {
        let v = prost_types::Value {
            kind: Some(Kind::StructValue(Struct {
                fields: [
                    ("a".to_string(), prost_types::Value { kind: Some(Kind::NumberValue(3.0)) }),
                    ("b".to_string(), prost_types::Value { kind: Some(Kind::ListValue(ListValue { values: vec![prost_types::Value { kind: Some(Kind::StringValue("x".into())) }] })) }),
                ]
                .into_iter()
                .collect(),
            })),
        };
        assert_eq!(from_proto(&v), serde_json::json!({ "a": 3, "b": ["x"] }));
    }

    #[test]
    fn ids_and_numbers() {
        assert_eq!(id_text(Some(&serde_json::json!("0x1"))).as_deref(), Some("0x1"));
        assert_eq!(id_text(Some(&serde_json::json!({ "id": "0x2" }))).as_deref(), Some("0x2"));
        assert_eq!(id_text(Some(&serde_json::json!(5))), None);
        assert_eq!(num(Some(&serde_json::json!("5"))), 5.0);
        assert_eq!(num(None), 0.0);
    }
}

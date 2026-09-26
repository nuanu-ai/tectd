use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

/// Interpret provider JSON only when every object key is unique, recursively.
pub(crate) fn decode_unique_json(bytes: &[u8]) -> serde_json::Result<Value> {
    serde_json::from_slice::<DuplicateCheckedValue>(bytes).map(|value| value.0)
}

struct DuplicateCheckedValue(Value);

impl<'de> Deserialize<'de> for DuplicateCheckedValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ValueVisitor).map(Self)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }
    fn visit_bool<E>(self, value: bool) -> std::result::Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E>(self, value: i64) -> std::result::Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_u64<E>(self, value: u64) -> std::result::Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_f64<E>(self, value: f64) -> std::result::Result<Value, E>
    where
        E: serde::de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite number"))
    }
    fn visit_str<E>(self, value: &str) -> std::result::Result<Value, E> {
        Ok(Value::String(value.into()))
    }
    fn visit_string<E>(self, value: String) -> std::result::Result<Value, E> {
        Ok(Value::String(value))
    }
    fn visit_none<E>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_unit<E>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_some<D>(self, deserializer: D) -> std::result::Result<Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        DuplicateCheckedValue::deserialize(deserializer).map(|value| value.0)
    }
    fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<DuplicateCheckedValue>()? {
            values.push(value.0);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A>(self, mut object: A) -> std::result::Result<Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some((key, value)) = object.next_entry::<String, DuplicateCheckedValue>()? {
            if values.insert(key, value.0).is_some() {
                return Err(serde::de::Error::custom("duplicate object key"));
            }
        }
        Ok(Value::Object(values))
    }
}

#[cfg(test)]
mod tests {
    use super::decode_unique_json;

    #[test]
    fn rejects_duplicate_keys_at_every_depth() {
        for raw in [
            r#"{"key":999999,"key":0}"#,
            r#"{"usage":{"input_tokens":999999,"input_tokens":0}}"#,
            r#"{"items":[{"key":1,"key":2}]}"#,
            r#"{"key":1,"\u006bey":2}"#,
        ] {
            assert!(decode_unique_json(raw.as_bytes()).is_err(), "{raw}");
        }
    }

    #[test]
    fn preserves_valid_json_values() {
        let raw = br#"{"usage":{"input_tokens":20,"output_tokens":30},"items":[null,true,-2,2.5,"text",{"key":1}]}"#;
        assert_eq!(
            decode_unique_json(raw).unwrap(),
            serde_json::from_slice::<serde_json::Value>(raw).unwrap()
        );
    }
}

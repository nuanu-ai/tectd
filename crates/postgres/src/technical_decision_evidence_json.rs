//! Reject duplicate object keys before serde's Value can discard them.
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::fmt;
use tect_domain::{Error, Result};

struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        d.deserialize_any(StrictVisitor)
    }
}
struct StrictVisitor;
impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Strict;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("unique-key JSON")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Strict, E> {
        Ok(Strict(v.into()))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Strict, E> {
        Ok(Strict(v.into()))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Strict, E> {
        Ok(Strict(v.into()))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Strict, E> {
        serde_json::Number::from_f64(v)
            .map(|n| Strict(Value::Number(n)))
            .ok_or_else(|| E::custom("invalid number"))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Strict, E> {
        Ok(Strict(v.into()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Strict, E> {
        Ok(Strict(v.into()))
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<Strict, E> {
        Ok(Strict(Value::Null))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> std::result::Result<Strict, A::Error> {
        let mut values = Vec::new();
        while let Some(Strict(value)) = a.next_element()? {
            values.push(value);
        }
        Ok(Strict(Value::Array(values)))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> std::result::Result<Strict, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some((key, Strict(value))) = a.next_entry::<String, Strict>()? {
            if values.insert(key, value).is_some() {
                return Err(de::Error::custom("duplicate key"));
            }
        }
        Ok(Strict(Value::Object(values)))
    }
}
pub(super) fn parse(body: &str) -> Result<Value> {
    serde_json::from_str::<Strict>(body)
        .map(|s| s.0)
        .map_err(|_| Error::Forbidden)
}

//! Parse once into a bounded tree. A plain `serde_json::Value` would silently
//! accept duplicate object keys, including keys nested inside asset metadata.
use std::fmt;

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

use crate::{Error, Result, ValidationLimits};

pub(super) fn parse(bytes: &[u8], limits: &ValidationLimits) -> Result<Value> {
    let mut budget = Budget {
        remaining: limits.max_json_nodes,
        depth: limits.max_json_depth,
    };
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let value = Seed {
        budget: &mut budget,
        depth: 0,
    }
    .deserialize(&mut decoder)
    .map_err(Error::Json)?;
    decoder.end().map_err(Error::Json)?;
    Ok(value)
}

struct Budget {
    remaining: usize,
    depth: usize,
}

struct Seed<'a> {
    budget: &'a mut Budget,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Value;

    fn deserialize<D: de::Deserializer<'de>>(
        self,
        decoder: D,
    ) -> std::result::Result<Value, D::Error> {
        if self.depth > self.budget.depth || self.budget.remaining == 0 {
            return Err(de::Error::custom(
                "JSON exceeds its nesting or value-count budget",
            ));
        }
        self.budget.remaining -= 1;
        decoder.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Seed<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_unit<E: de::Error>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> std::result::Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> std::result::Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> std::result::Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> std::result::Result<Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| de::Error::custom("non-finite JSON number"))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> std::result::Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> std::result::Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(Seed {
            budget: &mut *self.budget,
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> std::result::Result<Value, A::Error> {
        let mut values = Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                // Do not echo arbitrary uploaded keys into a log/error response.
                return Err(de::Error::custom("duplicate JSON object key"));
            }
            let value = object.next_value_seed(Seed {
                budget: &mut *self.budget,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicates_even_inside_a_nested_object() {
        assert!(
            parse(
                br#"{"outer":{"id":1,"id":2}}"#,
                &ValidationLimits::default()
            )
            .is_err()
        );
        assert!(parse(br#"{"id":1,"\u0069d":2}"#, &ValidationLimits::default()).is_err());
    }

    #[test]
    fn rejects_trailing_documents_and_invalid_numbers() {
        for input in [b"{} {}".as_slice(), b"NaN", b"1e999", b"{\"a\":1,}"] {
            assert!(parse(input, &ValidationLimits::default()).is_err());
        }
    }

    #[test]
    fn honors_depth_and_node_budgets() {
        let limits = ValidationLimits {
            max_json_depth: 2,
            ..Default::default()
        };
        assert!(parse(b"[[[[0]]]]", &limits).is_err());
        let limits = ValidationLimits {
            max_json_nodes: 3,
            ..Default::default()
        };
        assert!(parse(b"[1,2,3,4]", &limits).is_err());
    }
}

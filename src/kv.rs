use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::proto;

/// The `Value` enum represents the different types of values that can be stored in the key-value store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    String(String),
    Bytes(Box<[u8]>),
    Int(i64),
    UInt(u64),
    Float(f64),
    Bool(bool),
    Null,
}

/// Convert the `Value` enum into the protobuf format.
impl From<Value> for proto::Value {
    fn from(value: Value) -> Self {
        match value {
            Value::String(s) => proto::Value {
                value: Some(proto::value::Value::StringValue(s)),
            },
            Value::Bytes(b) => proto::Value {
                value: Some(proto::value::Value::BytesValue(b.to_vec())),
            },
            Value::Int(i) => proto::Value {
                value: Some(proto::value::Value::IntValue(i)),
            },
            Value::UInt(u) => proto::Value {
                value: Some(proto::value::Value::UintValue(u)),
            },
            Value::Float(f) => proto::Value {
                value: Some(proto::value::Value::FloatValue(f)),
            },
            Value::Bool(b) => proto::Value {
                value: Some(proto::value::Value::BoolValue(b)),
            },
            Value::Null => proto::Value { value: None },
        }
    }
}

/// Convert the protobuf format into the `Value` enum.
impl From<proto::Value> for Value {
    fn from(value: proto::Value) -> Self {
        match value.value {
            Some(proto::value::Value::StringValue(s)) => Value::String(s),
            Some(proto::value::Value::BytesValue(b)) => Value::Bytes(b.into_boxed_slice()),
            Some(proto::value::Value::IntValue(i)) => Value::Int(i),
            Some(proto::value::Value::UintValue(u)) => Value::UInt(u),
            Some(proto::value::Value::FloatValue(f)) => Value::Float(f),
            Some(proto::value::Value::BoolValue(b)) => Value::Bool(b),
            None => Value::Null,
        }
    }
}

/// Simple key-value store using [`DashMap`](https://docs.rs/dashmap/6.1.0/dashmap/).
pub struct KeyValueStore {
    data: DashMap<String, Value>,
}

impl Default for KeyValueStore {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyValueStore {
    pub fn new() -> Self {
        KeyValueStore {
            data: DashMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.data.get(key).map(|v| v.value().clone())
    }

    pub fn set(&self, key: String, value: Value) {
        self.data.insert(key, value);
    }

    pub fn delete(&self, key: &str) -> bool {
        self.data.remove(key).is_some()
    }

    pub fn list(&self) -> Vec<String> {
        self.data.iter().map(|item| item.key().clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_store() {
        let kv = KeyValueStore::new();
        assert_eq!(kv.list(), Vec::<String>::new());

        kv.set("name".to_string(), Value::String("Alice".to_string()));
        assert_eq!(kv.get("name"), Some(Value::String("Alice".to_string())));

        assert_eq!(kv.list(), vec!["name".to_string()]);
    }

    #[test]
    fn default_kv_store() {
        let kv = KeyValueStore::default();
        assert_eq!(kv.list(), Vec::<String>::new());

        kv.set("name".to_string(), Value::String("Alice".to_string()));
        assert_eq!(kv.get("name"), Some(Value::String("Alice".to_string())));

        assert_eq!(kv.list(), vec!["name".to_string()]);
    }
}

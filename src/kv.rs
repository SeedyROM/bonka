use dashmap::DashMap;
use serde::{Deserialize, Serialize};

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
    fn test_kv_store() {
        let kv = KeyValueStore::new();

        kv.set("name".to_string(), Value::String("Alice".to_string()));
        kv.set("age".to_string(), Value::Int(30));
        kv.set("is_student".to_string(), Value::Bool(true));

        assert_eq!(kv.get("name"), Some(Value::String("Alice".to_string())));
        assert_eq!(kv.get("age"), Some(Value::Int(30)));
        assert_eq!(kv.get("is_student"), Some(Value::Bool(true)));

        assert_eq!(kv.delete("name"), true);
        assert_eq!(kv.delete("name"), false);

        assert_eq!(kv.list(), vec!["age".to_string(), "is_student".to_string()]);
    }
}

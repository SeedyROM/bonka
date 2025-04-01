use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::kv::Value;

pub type Id = u64;
pub type Timestamp = u64;

/// The `Command` enum represents the different commands that can be executed against the key-value store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Get(String),
    Set(String, Value),
    Delete(String),
    List,
    Exit,
}

/// The `Result` enum represents the different types of results that can be returned from executing a command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Result {
    Value(Option<Value>),
    Success,
    Keys(Vec<String>),
    Error(String),
    Exit,
}

/// The `Metadata` type is a key-value store for additional information associated with a request or response.
pub type Metadata = HashMap<String, Value>;

/// The `Request` struct represents a request to the key-value store.
/// It contains an optional ID (to correlate with the request), a timestamp, the result of the command execution,
/// and optional metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: Option<Id>,
    pub timestamp: Timestamp,
    pub command: Command,
    pub metadata: Option<Metadata>,
}

/// The `Response` struct represents a response from the key-value store.
/// It contains an optional ID (to correlate with the request), a timestamp, the result of the command execution,
/// and optional metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: Option<Id>,
    pub timestamp: Timestamp,
    pub result: Result,
    pub metadata: Option<Metadata>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmp_serde::{Deserializer, Serializer};
    use std::io::Cursor;

    #[test]
    fn msgpack_serde() {
        let request = Request {
            id: Some(1),
            timestamp: 1234567890,
            command: Command::Set("key".to_string(), Value::String("value".to_string())),
            metadata: Some({
                let mut meta = HashMap::new();
                meta.insert("author".to_string(), Value::String("bonka".to_string()));
                meta.insert("version".to_string(), Value::UInt(1));
                meta
            }),
        };

        // Serialize the request
        let mut buf = Vec::new();
        request.serialize(&mut Serializer::new(&mut buf)).unwrap();

        // Deserialize the request
        let mut de = Deserializer::new(Cursor::new(buf));
        let deserialized_request: Request = Deserialize::deserialize(&mut de).unwrap();

        assert_eq!(request, deserialized_request);
    }
}

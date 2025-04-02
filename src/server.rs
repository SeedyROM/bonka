use bytes::Bytes;
use color_eyre::eyre::{self, Report};
use futures::{SinkExt, StreamExt};
use prost::Message;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::kv::KeyValueStore;
use crate::log;
use crate::proto::bonka::{CommandType, Request, Response, ResultType};
use crate::session::SessionManager;

/// Server state
struct ServerState {
    session_manager: SessionManager,
    kv_store: KeyValueStore,
}

/// Get current timestamp
fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Run the server
pub async fn run(host: impl Into<String>, port: u16) -> Result<(), Report> {
    // Get the address to bind to
    let addr = format!("{}:{}", host.into(), port);

    // Log server start
    log::info!("Starting bonka server on {}", &addr);

    // Initialize server state
    let state = Arc::new(Mutex::new(ServerState {
        session_manager: SessionManager::new(),
        kv_store: KeyValueStore::new(),
    }));

    // Start session cleanup task
    let cleanup_state = Arc::clone(&state);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.tick().await;
        loop {
            interval.tick().await;
            let mut state = cleanup_state.lock().unwrap();
            state
                .session_manager
                .cleanup_inactive_sessions(Duration::from_secs(1800)); // 30 minutes
            log::info!(
                "Cleaned up inactive sessions. Current count: {}",
                state.session_manager.session_count()
            );
        }
    });

    // Start the TCP server
    let listener = TcpListener::bind(&addr).await.map_err(eyre::Report::from)?;
    log::info!("Server listening on {}", addr);

    while let Ok((stream, addr)) = listener.accept().await {
        log::info!("New connection from: {}", addr);
        let client_state = Arc::clone(&state);

        // Handle each client in a separate task
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, addr.to_string(), client_state).await {
                log::error!("Error handling client {}: {}", addr, e);
            }
        });
    }

    Ok(())
}

/// Handle a client connection
///
/// This function processes a client connection, handling requests and sending responses.
async fn handle_client(
    stream: TcpStream,
    addr: String,
    state: Arc<Mutex<ServerState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create a session for this client
    let session_id = {
        let mut server_state = state.lock().unwrap();
        let session = server_state.session_manager.create_session(addr.clone());
        session.id
    };

    log::info!("Created session {} for client {}", session_id, addr);

    // Use LengthDelimitedCodec for framing
    let mut framed = Framed::new(stream, LengthDelimitedCodec::new());

    // Process client messages
    while let Some(result) = framed.next().await {
        match result {
            Ok(bytes) => {
                // Deserialize request using MessagePack
                let request: Request = match Request::decode(bytes.as_ref()) {
                    Ok(req) => req,
                    Err(e) => {
                        log::error!("Failed to deserialize request: {}", e);

                        // Send error response
                        let error_response = Response {
                            id: None,
                            timestamp: get_timestamp(),
                            result_type: ResultType::ResultError as i32,
                            error: Some("Invalid request format".to_string()),
                            ..Default::default()
                        };

                        send_response(&mut framed, &error_response).await?;
                        continue;
                    }
                };

                // Update session activity
                {
                    let mut server_state = state.lock().unwrap();
                    server_state.session_manager.touch_session(session_id);
                }

                // Process the command
                let response = process_command(request, &state).await;

                // Send response
                send_response(&mut framed, &response).await?;

                // Check if client is exiting
                if response.result_type() == ResultType::ResultExit {
                    log::info!("Client {} requested exit", addr);
                    break;
                }
            }
            Err(e) => {
                log::error!("Error reading from client {}: {}", addr, e);
                break;
            }
        }
    }

    // Clean up session
    {
        let mut server_state = state.lock().unwrap();
        server_state.session_manager.remove_session(session_id);
    }

    log::info!("Connection closed for client {}", addr);
    Ok(())
}

/// Process a command and return a response
///
/// This function processes a command from a client request and returns a response.
async fn process_command(request: Request, state: &Arc<Mutex<ServerState>>) -> Response {
    let server_state = state.lock().unwrap();
    let kv_store = &server_state.kv_store;

    match request.command_type() {
        CommandType::CommandGet => {
            if let Some(key) = request.key.clone() {
                // Convert Option<Value> to Value and handle None case properly
                match kv_store.get(&key) {
                    Some(value) => Response {
                        id: request.id,
                        timestamp: get_timestamp(),
                        result_type: ResultType::ResultValue as i32,
                        value: Some(value.into()),
                        ..Default::default()
                    },
                    None => Response {
                        id: request.id,
                        timestamp: get_timestamp(),
                        result_type: ResultType::ResultValue as i32,
                        value: None,
                        ..Default::default()
                    },
                }
            } else {
                Response {
                    id: request.id,
                    timestamp: get_timestamp(),
                    result_type: ResultType::ResultError as i32,
                    error: Some("Key not provided".to_string()),
                    ..Default::default()
                }
            }
        }
        CommandType::CommandSet => {
            if let Some(key) = request.key.clone() {
                if let Some(value) = request.value.clone() {
                    kv_store.set(key, value.into());
                    Response {
                        id: request.id,
                        timestamp: get_timestamp(),
                        result_type: ResultType::ResultSuccess as i32,
                        ..Default::default()
                    }
                } else {
                    Response {
                        id: request.id,
                        timestamp: get_timestamp(),
                        result_type: ResultType::ResultError as i32,
                        error: Some("Value not provided".to_string()),
                        ..Default::default()
                    }
                }
            } else {
                Response {
                    id: request.id,
                    timestamp: get_timestamp(),
                    result_type: ResultType::ResultError as i32,
                    error: Some("Key not provided".to_string()),
                    ..Default::default()
                }
            }
        }
        CommandType::CommandDelete => {
            if let Some(key) = request.key.clone() {
                if kv_store.delete(&key) {
                    Response {
                        id: request.id,
                        timestamp: get_timestamp(),
                        result_type: ResultType::ResultSuccess as i32,
                        ..Default::default()
                    }
                } else {
                    Response {
                        id: request.id,
                        timestamp: get_timestamp(),
                        result_type: ResultType::ResultError as i32,
                        error: Some(format!("Key '{}' not found", key)),
                        ..Default::default()
                    }
                }
            } else {
                Response {
                    id: request.id,
                    timestamp: get_timestamp(),
                    result_type: ResultType::ResultError as i32,
                    error: Some("Key not provided".to_string()),
                    ..Default::default()
                }
            }
        }
        CommandType::CommandList => {
            let keys = kv_store.list();
            Response {
                id: request.id,
                timestamp: get_timestamp(),
                result_type: ResultType::ResultKeys as i32,
                keys,
                ..Default::default()
            }
        }
        CommandType::CommandExit => Response {
            id: request.id,
            timestamp: get_timestamp(),
            result_type: ResultType::ResultExit as i32,
            ..Default::default()
        },
        _ => Response {
            id: request.id,
            timestamp: get_timestamp(),
            result_type: ResultType::ResultError as i32,
            error: Some("Unknown command".to_string()),
            ..Default::default()
        },
    }
}

/// Send a response to the client
///
/// This function serializes the response using MessagePack and sends it to the client.
async fn send_response(
    framed: &mut Framed<TcpStream, LengthDelimitedCodec>,
    response: &Response,
) -> Result<(), Box<dyn std::error::Error>> {
    // Serialize response using protobuf
    let encoded = response.encode_to_vec();
    // Send the response
    framed.send(Bytes::from(encoded)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv;
    use crate::proto::bonka::{self, CommandType, ResultType};
    use prost::Message;

    // Test server setup and teardown
    struct TestServer {
        host: String,
        port: u16,
        server_handle: Option<tokio::task::JoinHandle<()>>,
    }

    impl TestServer {
        fn new(port: u16) -> Self {
            TestServer {
                host: "127.0.0.1".to_string(),
                port,
                server_handle: None,
            }
        }

        async fn start(&mut self) {
            let host = self.host.clone();
            let port = self.port;

            self.server_handle = Some(tokio::spawn(async move {
                if let Err(e) = run(host, port).await {
                    eprintln!("Server error: {}", e);
                }
            }));

            // Wait a moment for the server to start
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        async fn stop(&mut self) {
            if let Some(handle) = self.server_handle.take() {
                handle.abort();
            }
        }
    }

    // Helper function to create a client connection
    async fn connect_client(host: &str, port: u16) -> Framed<TcpStream, LengthDelimitedCodec> {
        let addr = format!("{}:{}", host, port);
        let retries = 5;
        let mut attempt = 0;

        loop {
            match TcpStream::connect(&addr).await {
                Ok(stream) => return Framed::new(stream, LengthDelimitedCodec::new()),
                Err(e) => {
                    attempt += 1;
                    if attempt >= retries {
                        panic!(
                            "Failed to connect to server after {} attempts: {}",
                            retries, e
                        );
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    // Helper function to create a protobuf Value from a kv::Value
    fn create_proto_value(value: kv::Value) -> bonka::Value {
        value.into()
    }

    // Helper function to send a command and get response
    async fn send_command(
        framed: &mut Framed<TcpStream, LengthDelimitedCodec>,
        command_type: CommandType,
        key: Option<String>,
        value: Option<bonka::Value>,
    ) -> bonka::Response {
        let request = bonka::Request {
            id: Some(1), // Use a test ID
            timestamp: get_timestamp(),
            command_type: command_type as i32,
            key,
            value,
            metadata: Default::default(),
        };

        // Serialize request using protobuf
        let encoded = request.encode_to_vec();

        // Send request
        framed
            .send(Bytes::from(encoded))
            .await
            .expect("Failed to send request");

        // Receive response
        let bytes = framed
            .next()
            .await
            .expect("No response received")
            .expect("Failed to receive response");

        // Deserialize response
        bonka::Response::decode(bytes.as_ref()).expect("Failed to deserialize response")
    }

    #[tokio::test]
    async fn set_and_get() {
        let mut server = TestServer::new(8001);
        server.start().await;

        // Connect client
        let mut client = connect_client(&server.host, server.port).await;

        // Set a key
        let set_response = send_command(
            &mut client,
            CommandType::CommandSet,
            Some("test-key".to_string()),
            Some(create_proto_value(kv::Value::String(
                "test-value".to_string(),
            ))),
        )
        .await;

        // Check set was successful
        assert_eq!(set_response.result_type(), ResultType::ResultSuccess);

        // Get the key
        let get_response = send_command(
            &mut client,
            CommandType::CommandGet,
            Some("test-key".to_string()),
            None,
        )
        .await;

        // Check the value is correct
        assert_eq!(get_response.result_type(), ResultType::ResultValue);
        assert!(get_response.value.is_some());
        let proto_value = get_response.value.unwrap();

        // Convert to a kv::Value and check
        let kv_value: kv::Value = proto_value.into();
        if let kv::Value::String(value) = kv_value {
            assert_eq!(value, "test-value");
        } else {
            panic!("Expected String value, got {:?}", kv_value);
        }

        // Clean up
        server.stop().await;
    }

    #[tokio::test]
    async fn delete() {
        let mut server = TestServer::new(8002);
        server.start().await;

        // Connect client
        let mut client = connect_client(&server.host, server.port).await;

        // Set a key
        let _ = send_command(
            &mut client,
            CommandType::CommandSet,
            Some("delete-key".to_string()),
            Some(create_proto_value(kv::Value::String("value".to_string()))),
        )
        .await;

        // Delete the key
        let delete_response = send_command(
            &mut client,
            CommandType::CommandDelete,
            Some("delete-key".to_string()),
            None,
        )
        .await;

        // Check delete was successful
        assert_eq!(delete_response.result_type(), ResultType::ResultSuccess);

        // Try to get the deleted key
        let get_response = send_command(
            &mut client,
            CommandType::CommandGet,
            Some("delete-key".to_string()),
            None,
        )
        .await;

        // Key should not exist
        assert_eq!(get_response.result_type(), ResultType::ResultValue);
        assert!(get_response.value.is_none());

        // Clean up
        server.stop().await;
    }

    #[tokio::test]
    async fn delete_nonexistent() {
        let mut server = TestServer::new(8003);
        server.start().await;

        // Connect client
        let mut client = connect_client(&server.host, server.port).await;

        // Try to delete a non-existent key
        let delete_response = send_command(
            &mut client,
            CommandType::CommandDelete,
            Some("nonexistent-key".to_string()),
            None,
        )
        .await;

        // Should get an error
        assert_eq!(delete_response.result_type(), ResultType::ResultError);
        assert!(delete_response.error.is_some());
        assert!(delete_response.error.unwrap().contains("not found"));

        // Clean up
        server.stop().await;
    }

    #[tokio::test]
    async fn list_keys() {
        let mut server = TestServer::new(8004);
        server.start().await;

        // Connect client
        let mut client = connect_client(&server.host, server.port).await;

        // Add multiple keys
        let keys = vec!["key1", "key2", "key3"];
        for key in &keys {
            let _ = send_command(
                &mut client,
                CommandType::CommandSet,
                Some(key.to_string()),
                Some(create_proto_value(kv::Value::String(format!(
                    "value-{}",
                    key
                )))),
            )
            .await;
        }

        // List all keys
        let list_response = send_command(&mut client, CommandType::CommandList, None, None).await;

        // Check that all our keys are listed
        assert_eq!(list_response.result_type(), ResultType::ResultKeys);
        assert!(!list_response.keys.is_empty());

        // Convert Vec<String> to Vec<&str> for easier comparison
        let response_keys_str: Vec<&str> = list_response.keys.iter().map(|s| s.as_str()).collect();

        // Check each key is present
        for key in keys {
            assert!(response_keys_str.contains(&key));
        }

        // Clean up
        server.stop().await;
    }

    #[tokio::test]
    async fn different_value_types() {
        let mut server = TestServer::new(8005);
        server.start().await;

        // Connect client
        let mut client = connect_client(&server.host, server.port).await;

        // Setup test cases with KV values
        let test_values = vec![
            ("string-key", kv::Value::String("string-value".to_string())),
            ("int-key", kv::Value::Int(42)),
            ("float-key", kv::Value::Float(1.337)),
            ("bool-key", kv::Value::Bool(true)),
            ("null-key", kv::Value::Null),
        ];

        // Set each value
        for (key, value) in &test_values {
            let proto_value = create_proto_value(value.clone());

            let set_response = send_command(
                &mut client,
                CommandType::CommandSet,
                Some(key.to_string()),
                Some(proto_value),
            )
            .await;

            assert_eq!(set_response.result_type(), ResultType::ResultSuccess);
        }

        // Get and verify each value
        for (key, expected_value) in test_values {
            let get_response = send_command(
                &mut client,
                CommandType::CommandGet,
                Some(key.to_string()),
                None,
            )
            .await;

            assert_eq!(get_response.result_type(), ResultType::ResultValue);
            assert!(get_response.value.is_some());

            // Convert to a kv::Value and compare
            let kv_value: kv::Value = get_response.value.unwrap().into();
            assert_eq!(kv_value, expected_value);
        }

        // Clean up
        server.stop().await;
    }

    #[tokio::test]
    async fn concurrent_clients() {
        let mut server = TestServer::new(8006);
        server.start().await;

        // Number of concurrent clients
        let client_count = 5;
        let operations_per_client = 10;

        // Spawn multiple clients
        let handles: Vec<_> = (0..client_count)
            .map(|client_id| {
                tokio::spawn(async move {
                    // Connect client
                    let host = "127.0.0.1";
                    let port = 8006;
                    let mut client = connect_client(host, port).await;

                    // Each client performs multiple operations
                    for i in 0..operations_per_client {
                        let key = format!("client{}-key{}", client_id, i);
                        let value = format!("value{}-{}", client_id, i);

                        // Set a key
                        let set_response = send_command(
                            &mut client,
                            CommandType::CommandSet,
                            Some(key.clone()),
                            Some(create_proto_value(kv::Value::String(value.clone()))),
                        )
                        .await;

                        assert_eq!(set_response.result_type(), ResultType::ResultSuccess);

                        // Get the key back
                        let get_response =
                            send_command(&mut client, CommandType::CommandGet, Some(key), None)
                                .await;

                        assert_eq!(get_response.result_type(), ResultType::ResultValue);
                        assert!(get_response.value.is_some());

                        let kv_value: kv::Value = get_response.value.unwrap().into();
                        if let kv::Value::String(response_value) = kv_value {
                            assert_eq!(response_value, value);
                        } else {
                            panic!("Expected String value, got {:?}", kv_value);
                        }
                    }
                })
            })
            .collect();

        // Wait for all clients to complete
        for handle in handles {
            handle.await.expect("Client task failed");
        }

        // Connect a new client to verify all keys are present
        let mut verification_client = connect_client(&server.host, server.port).await;
        let list_response = send_command(
            &mut verification_client,
            CommandType::CommandList,
            None,
            None,
        )
        .await;

        assert_eq!(list_response.result_type(), ResultType::ResultKeys);
        assert_eq!(
            list_response.keys.len() as u32,
            client_count * operations_per_client
        );

        // Verify each expected key exists
        for client_id in 0..client_count {
            for i in 0..operations_per_client {
                let expected_key = format!("client{}-key{}", client_id, i);
                assert!(list_response.keys.contains(&expected_key));
            }
        }

        // Clean up
        server.stop().await;
    }

    #[tokio::test]
    async fn exit_command() {
        let mut server = TestServer::new(8007);
        server.start().await;

        // Connect client
        let mut client = connect_client(&server.host, server.port).await;

        // Send exit command
        let exit_response = send_command(&mut client, CommandType::CommandExit, None, None).await;

        // Check exit response
        assert_eq!(exit_response.result_type(), ResultType::ResultExit);

        // Try to send another command, should fail as connection should be closed
        let buf = client.next().await;
        assert!(buf.is_none()); // Connection should be closed

        // Clean up
        server.stop().await;
    }

    #[tokio::test]
    async fn invalid_message_format() {
        let mut server = TestServer::new(8008);
        server.start().await;

        // Connect client
        let mut client = connect_client(&server.host, server.port).await;

        // Send invalid data (not a valid protobuf Request)
        let invalid_data = Bytes::from(vec![0, 1, 2, 3]);
        client
            .send(invalid_data)
            .await
            .expect("Failed to send invalid data");

        // Server should respond with an error
        let response_bytes = client
            .next()
            .await
            .expect("No response received")
            .expect("Failed to receive response");

        let response = bonka::Response::decode(response_bytes.as_ref())
            .expect("Failed to deserialize error response");

        assert_eq!(response.result_type(), ResultType::ResultError);

        // Clean up
        server.stop().await;
    }

    #[tokio::test]
    async fn session_management() {
        let mut server = TestServer::new(8009);
        server.start().await;

        // Connect first client
        let mut client1 = connect_client(&server.host, server.port).await;

        // Set a key using first client
        let set_response = send_command(
            &mut client1,
            CommandType::CommandSet,
            Some("session-key".to_string()),
            Some(create_proto_value(kv::Value::String(
                "session-value".to_string(),
            ))),
        )
        .await;

        assert_eq!(set_response.result_type(), ResultType::ResultSuccess);

        // Connect second client
        let mut client2 = connect_client(&server.host, server.port).await;

        // Get the key using second client (should be visible to all clients)
        let get_response = send_command(
            &mut client2,
            CommandType::CommandGet,
            Some("session-key".to_string()),
            None,
        )
        .await;

        assert_eq!(get_response.result_type(), ResultType::ResultValue);
        assert!(get_response.value.is_some());

        let kv_value: kv::Value = get_response.value.unwrap().into();
        if let kv::Value::String(value) = kv_value {
            assert_eq!(value, "session-value");
        } else {
            panic!("Expected String value, got {:?}", kv_value);
        }

        // Close first client with Exit command
        let _ = send_command(&mut client1, CommandType::CommandExit, None, None).await;

        // Key should still be accessible from second client
        let get_response = send_command(
            &mut client2,
            CommandType::CommandGet,
            Some("session-key".to_string()),
            None,
        )
        .await;

        assert_eq!(get_response.result_type(), ResultType::ResultValue);
        assert!(get_response.value.is_some());

        let kv_value: kv::Value = get_response.value.unwrap().into();
        if let kv::Value::String(value) = kv_value {
            assert_eq!(value, "session-value");
        } else {
            panic!("Expected String value, got {:?}", kv_value);
        }

        // Clean up
        server.stop().await;
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     use crate::kv::Value;

//     // Test server setup and teardown
//     struct TestServer {
//         host: String,
//         port: u16,
//         server_handle: Option<tokio::task::JoinHandle<()>>,
//     }

//     impl TestServer {
//         fn new(port: u16) -> Self {
//             TestServer {
//                 host: "127.0.0.1".to_string(),
//                 port,
//                 server_handle: None,
//             }
//         }

//         async fn start(&mut self) {
//             let host = self.host.clone();
//             let port = self.port;

//             self.server_handle = Some(tokio::spawn(async move {
//                 if let Err(e) = run(host, port).await {
//                     eprintln!("Server error: {}", e);
//                 }
//             }));

//             // Wait a moment for the server to start
//             tokio::time::sleep(Duration::from_millis(100)).await;
//         }

//         async fn stop(&mut self) {
//             if let Some(handle) = self.server_handle.take() {
//                 handle.abort();
//             }
//         }
//     }

//     // Helper function to create a client connection
//     async fn connect_client(host: &str, port: u16) -> Framed<TcpStream, LengthDelimitedCodec> {
//         let addr = format!("{}:{}", host, port);
//         let retries = 5;
//         let mut attempt = 0;

//         loop {
//             match TcpStream::connect(&addr).await {
//                 Ok(stream) => return Framed::new(stream, LengthDelimitedCodec::new()),
//                 Err(e) => {
//                     attempt += 1;
//                     if attempt >= retries {
//                         panic!(
//                             "Failed to connect to server after {} attempts: {}",
//                             retries, e
//                         );
//                     }
//                     tokio::time::sleep(Duration::from_millis(100)).await;
//                 }
//             }
//         }
//     }

//     // Helper function to send a command and get response
//     async fn send_command(
//         framed: &mut Framed<TcpStream, LengthDelimitedCodec>,
//         command: Command,
//     ) -> Response {
//         let request = Request {
//             id: None,
//             timestamp: get_timestamp(),
//             command,
//             metadata: None,
//         };

//         // Serialize request
//         let mut buf = Vec::new();
//         request
//             .serialize(&mut rmp_serde::Serializer::new(&mut buf))
//             .expect("Failed to serialize request");

//         // Send request
//         framed
//             .send(Bytes::from(buf))
//             .await
//             .expect("Failed to send request");

//         // Receive response
//         let bytes = framed
//             .next()
//             .await
//             .expect("No response received")
//             .expect("Failed to receive response");

//         // Deserialize response
//         rmp_serde::from_slice(&bytes).expect("Failed to deserialize response")
//     }

//     #[tokio::test]
//     async fn set_and_get() {
//         let mut server = TestServer::new(8001);
//         server.start().await;

//         // Connect client
//         let mut client = connect_client(&server.host, server.port).await;

//         // Set a key
//         let set_cmd = Command::Set(
//             "test-key".to_string(),
//             Value::String("test-value".to_string()),
//         );
//         let set_response = send_command(&mut client, set_cmd).await;

//         // Check set was successful
//         assert!(matches!(set_response.result, ProtocolResult::Success));

//         // Get the key
//         let get_cmd = Command::Get("test-key".to_string());
//         let get_response = send_command(&mut client, get_cmd).await;

//         // Check the value is correct
//         if let ProtocolResult::Value(Some(Value::String(value))) = get_response.result {
//             assert_eq!(value, "test-value");
//         } else {
//             panic!("Expected Value::String, got {:?}", get_response.result);
//         }

//         // Clean up
//         server.stop().await;
//     }

//     #[tokio::test]
//     async fn delete() {
//         let mut server = TestServer::new(8002);
//         server.start().await;

//         // Connect client
//         let mut client = connect_client(&server.host, server.port).await;

//         // Set a key
//         let set_cmd = Command::Set("delete-key".to_string(), Value::String("value".to_string()));
//         let _ = send_command(&mut client, set_cmd).await;

//         // Delete the key
//         let delete_cmd = Command::Delete("delete-key".to_string());
//         let delete_response = send_command(&mut client, delete_cmd).await;

//         // Check delete was successful
//         assert!(matches!(delete_response.result, ProtocolResult::Success));

//         // Try to get the deleted key
//         let get_cmd = Command::Get("delete-key".to_string());
//         let get_response = send_command(&mut client, get_cmd).await;

//         // Key should not exist
//         if let ProtocolResult::Value(value) = get_response.result {
//             assert!(value.is_none());
//         } else {
//             panic!(
//                 "Expected ProtocolResult::Value(None), got {:?}",
//                 get_response.result
//             );
//         }

//         // Clean up
//         server.stop().await;
//     }

//     #[tokio::test]
//     async fn delete_nonexistent() {
//         let mut server = TestServer::new(8003);
//         server.start().await;

//         // Connect client
//         let mut client = connect_client(&server.host, server.port).await;

//         // Try to delete a non-existent key
//         let delete_cmd = Command::Delete("nonexistent-key".to_string());
//         let delete_response = send_command(&mut client, delete_cmd).await;

//         // Should get an error
//         if let ProtocolResult::Error(err) = delete_response.result {
//             assert!(err.contains("not found"));
//         } else {
//             panic!(
//                 "Expected ProtocolResult::Error, got {:?}",
//                 delete_response.result
//             );
//         }

//         // Clean up
//         server.stop().await;
//     }

//     #[tokio::test]
//     async fn list_keys() {
//         let mut server = TestServer::new(8004);
//         server.start().await;

//         // Connect client
//         let mut client = connect_client(&server.host, server.port).await;

//         // Add multiple keys
//         let keys = vec!["key1", "key2", "key3"];
//         for key in &keys {
//             let set_cmd = Command::Set(key.to_string(), Value::String(format!("value-{}", key)));
//             let _ = send_command(&mut client, set_cmd).await;
//         }

//         // List all keys
//         let list_cmd = Command::List;
//         let list_response = send_command(&mut client, list_cmd).await;

//         // Check that all our keys are listed
//         if let ProtocolResult::Keys(response_keys) = list_response.result {
//             // Convert Vec<String> to Vec<&str> for easier comparison
//             let response_keys_str: Vec<&str> = response_keys.iter().map(|s| s.as_str()).collect();

//             // Check each key is present
//             for key in keys {
//                 assert!(response_keys_str.contains(&key));
//             }
//         } else {
//             panic!(
//                 "Expected ProtocolResult::Keys, got {:?}",
//                 list_response.result
//             );
//         }

//         // Clean up
//         server.stop().await;
//     }

//     #[tokio::test]
//     async fn different_value_types() {
//         let mut server = TestServer::new(8005);
//         server.start().await;

//         // Connect client
//         let mut client = connect_client(&server.host, server.port).await;

//         // Test different value types
//         let test_values = vec![
//             ("string-key", Value::String("string-value".to_string())),
//             ("int-key", Value::Int(42)),
//             ("float-key", Value::Float(3.14)),
//             ("bool-key", Value::Bool(true)),
//             ("null-key", Value::Null),
//             // You could add more complex types like arrays and maps if your Value enum supports them
//         ];

//         // Set each value
//         for (key, value) in &test_values {
//             let set_cmd = Command::Set(key.to_string(), value.clone());
//             let set_response = send_command(&mut client, set_cmd).await;
//             assert!(matches!(set_response.result, ProtocolResult::Success));
//         }

//         // Get and verify each value
//         for (key, expected_value) in test_values {
//             let get_cmd = Command::Get(key.to_string());
//             let get_response = send_command(&mut client, get_cmd).await;

//             if let ProtocolResult::Value(Some(value)) = get_response.result {
//                 assert_eq!(value, expected_value);
//             } else {
//                 panic!(
//                     "Expected value for key {}, got {:?}",
//                     key, get_response.result
//                 );
//             }
//         }

//         // Clean up
//         server.stop().await;
//     }

//     #[tokio::test]
//     async fn concurrent_clients() {
//         let mut server = TestServer::new(8006);
//         server.start().await;

//         // Number of concurrent clients
//         let client_count = 5;
//         let operations_per_client = 10;

//         // Spawn multiple clients
//         let handles: Vec<_> = (0..client_count)
//             .map(|client_id| {
//                 tokio::spawn(async move {
//                     // Connect client
//                     let host = "127.0.0.1";
//                     let port = 8006;
//                     let mut client = connect_client(host, port).await;

//                     // Each client performs multiple operations
//                     for i in 0..operations_per_client {
//                         let key = format!("client{}-key{}", client_id, i);
//                         let value = format!("value{}-{}", client_id, i);

//                         // Set a key
//                         let set_cmd = Command::Set(key.clone(), Value::String(value.clone()));
//                         let set_response = send_command(&mut client, set_cmd).await;
//                         assert!(matches!(set_response.result, ProtocolResult::Success));

//                         // Get the key back
//                         let get_cmd = Command::Get(key);
//                         let get_response = send_command(&mut client, get_cmd).await;

//                         if let ProtocolResult::Value(Some(Value::String(response_value))) =
//                             get_response.result
//                         {
//                             assert_eq!(response_value, value);
//                         } else {
//                             panic!("Expected Value::String, got {:?}", get_response.result);
//                         }
//                     }
//                 })
//             })
//             .collect();

//         // Wait for all clients to complete
//         for handle in handles {
//             handle.await.expect("Client task failed");
//         }

//         // Connect a new client to verify all keys are present
//         let mut verification_client = connect_client(&server.host, server.port).await;
//         let list_cmd = Command::List;
//         let list_response = send_command(&mut verification_client, list_cmd).await;

//         if let ProtocolResult::Keys(keys) = list_response.result {
//             assert_eq!(keys.len(), client_count * operations_per_client);

//             // Verify each expected key exists
//             for client_id in 0..client_count {
//                 for i in 0..operations_per_client {
//                     let expected_key = format!("client{}-key{}", client_id, i);
//                     assert!(keys.contains(&expected_key));
//                 }
//             }
//         } else {
//             panic!(
//                 "Expected ProtocolResult::Keys, got {:?}",
//                 list_response.result
//             );
//         }

//         // Clean up
//         server.stop().await;
//     }

//     #[tokio::test]
//     async fn exit_command() {
//         let mut server = TestServer::new(8007);
//         server.start().await;

//         // Connect client
//         let mut client = connect_client(&server.host, server.port).await;

//         // Send exit command
//         let exit_cmd = Command::Exit;
//         let exit_response = send_command(&mut client, exit_cmd).await;

//         // Check exit response
//         assert!(matches!(exit_response.result, ProtocolResult::Exit));

//         // Try to send another command, should fail as connection should be closed
//         let buf = client.next().await;
//         assert!(buf.is_none()); // Connection should be closed

//         // Clean up
//         server.stop().await;
//     }

//     #[tokio::test]
//     async fn invalid_message_format() {
//         let mut server = TestServer::new(8008);
//         server.start().await;

//         // Connect client
//         let mut client = connect_client(&server.host, server.port).await;

//         // Send invalid data (not a valid MessagePack serialized Request)
//         let invalid_data = Bytes::from(vec![0, 1, 2, 3]);
//         client
//             .send(invalid_data)
//             .await
//             .expect("Failed to send invalid data");

//         // Server should respond with an error
//         let response_bytes = client
//             .next()
//             .await
//             .expect("No response received")
//             .expect("Failed to receive response");
//         let response: Response =
//             rmp_serde::from_slice(&response_bytes).expect("Failed to deserialize error response");

//         assert!(matches!(response.result, ProtocolResult::Error(_)));

//         // Clean up
//         server.stop().await;
//     }

//     #[tokio::test]
//     async fn session_management() {
//         let mut server = TestServer::new(8009);
//         server.start().await;

//         // Connect first client
//         let mut client1 = connect_client(&server.host, server.port).await;

//         // Set a key using first client
//         let set_cmd = Command::Set(
//             "session-key".to_string(),
//             Value::String("session-value".to_string()),
//         );
//         let set_response = send_command(&mut client1, set_cmd).await;
//         assert!(matches!(set_response.result, ProtocolResult::Success));

//         // Connect second client
//         let mut client2 = connect_client(&server.host, server.port).await;

//         // Get the key using second client (should be visible to all clients)
//         let get_cmd = Command::Get("session-key".to_string());
//         let get_response = send_command(&mut client2, get_cmd).await;

//         if let ProtocolResult::Value(Some(Value::String(value))) = get_response.result {
//             assert_eq!(value, "session-value");
//         } else {
//             panic!("Expected Value::String, got {:?}", get_response.result);
//         }

//         // Close first client with Exit command
//         let exit_cmd = Command::Exit;
//         let _ = send_command(&mut client1, exit_cmd).await;

//         // Key should still be accessible from second client
//         let get_cmd = Command::Get("session-key".to_string());
//         let get_response = send_command(&mut client2, get_cmd).await;

//         if let ProtocolResult::Value(Some(Value::String(value))) = get_response.result {
//             assert_eq!(value, "session-value");
//         } else {
//             panic!("Expected Value::String, got {:?}", get_response.result);
//         }

//         // Clean up
//         server.stop().await;
//     }
// }

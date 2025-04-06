use bytes::Bytes;
use color_eyre::eyre;
use futures::{SinkExt, StreamExt};
use prost::Message;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::kv::{self, KeyValueStore};
use crate::log;
use crate::proto::{CommandType, Request, Response, ResultType};
use crate::session::SessionManager;

/// Error type for the server
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("Server failed to set max FD limits: {0}")]
    MaxFdLimits(#[source] eyre::Report),
    #[error("Socket bind error: {0}")]
    SocketBind(#[source] std::io::Error),
    #[error("Socket configuration error: {0}")]
    SocketConfig(#[source] std::io::Error),
    #[error("Failed to acquire semaphore permit: {0}")]
    SemaphoreAcquire(#[source] tokio::sync::AcquireError),
    #[error("Failed to connect to server: {0}")]
    Connection(#[source] std::io::Error),
    #[error("Failed to send response: {0}")]
    SendResponse(#[source] std::io::Error),
    #[error("Failed to lock server state")]
    LockingState,
}

/// Extension trait for socket configuration
trait SocketConfigExt {
    fn configure<F, T>(&self, f: F) -> Result<T, ServerError>
    where
        F: FnOnce(&Self) -> std::io::Result<T>;
}

impl SocketConfigExt for socket2::Socket {
    fn configure<F, T>(&self, f: F) -> Result<T, ServerError>
    where
        F: FnOnce(&Self) -> std::io::Result<T>,
    {
        f(self).map_err(ServerError::SocketBind)
    }
}

impl SocketConfigExt for TcpStream {
    fn configure<F, T>(&self, f: F) -> Result<T, ServerError>
    where
        F: FnOnce(&Self) -> std::io::Result<T>,
    {
        f(self).map_err(ServerError::SocketBind)
    }
}

struct ServerState {
    session_manager: SessionManager,
    kv_store: KeyValueStore,
}

#[inline(always)]
fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Failed to get system time, RIP")
        .as_secs()
}

// ===============================================
// Helpers for creating responses / handling commands
// ===============================================

#[inline(always)]
fn create_success_response(id: Option<u64>) -> Response {
    Response {
        id,
        timestamp: get_timestamp(),
        result_type: ResultType::ResultSuccess as i32,
        ..Default::default()
    }
}

#[inline(always)]
fn create_value_response(id: Option<u64>, value: Option<kv::Value>) -> Response {
    Response {
        id,
        timestamp: get_timestamp(),
        result_type: ResultType::ResultValue as i32,
        value: value.map(|v| v.into()),
        ..Default::default()
    }
}

#[inline(always)]
fn create_error_response(id: Option<u64>, message: String) -> Response {
    Response {
        id,
        timestamp: get_timestamp(),
        result_type: ResultType::ResultError as i32,
        error: Some(message),
        ..Default::default()
    }
}

#[inline(always)]
fn create_keys_response(id: Option<u64>, keys: Vec<String>) -> Response {
    Response {
        id,
        timestamp: get_timestamp(),
        result_type: ResultType::ResultKeys as i32,
        keys,
        ..Default::default()
    }
}

#[inline(always)]
fn create_exit_response(id: Option<u64>) -> Response {
    Response {
        id,
        timestamp: get_timestamp(),
        result_type: ResultType::ResultExit as i32,
        ..Default::default()
    }
}

#[inline(always)]
fn handle_get_command(request: &Request, kv_store: &KeyValueStore) -> Response {
    match &request.key {
        Some(key) => create_value_response(request.id, kv_store.get(key)),
        None => create_error_response(request.id, "Key not provided".to_string()),
    }
}

#[inline(always)]
fn handle_set_command(request: &Request, kv_store: &KeyValueStore) -> Response {
    match (&request.key, &request.value) {
        (Some(key), Some(value)) => {
            kv_store.set(key.clone(), value.clone().into());
            create_success_response(request.id)
        }
        (None, _) => create_error_response(request.id, "Key not provided".to_string()),
        (_, None) => create_error_response(request.id, "Value not provided".to_string()),
    }
}

#[inline(always)]
fn handle_delete_command(request: &Request, kv_store: &KeyValueStore) -> Response {
    match &request.key {
        Some(key) => {
            if kv_store.delete(key) {
                create_success_response(request.id)
            } else {
                create_error_response(request.id, format!("Key '{}' not found", key))
            }
        }
        None => create_error_response(request.id, "Key not provided".to_string()),
    }
}

#[inline(always)]
fn handle_list_command(request: &Request, kv_store: &KeyValueStore) -> Response {
    let keys = kv_store.list();
    create_keys_response(request.id, keys)
}

// ===============================================
// End response helpers
// ===============================================

fn set_file_descriptor_limits() {
    // Try to get the current limits
    let mut rlimit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };

    // Get current limits
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlimit) } == 0 {
        // Try to set higher limits if needed
        let desired_soft_limit = 65536;

        if rlimit.rlim_max < desired_soft_limit as libc::rlim_t {
            // Log that we can't set as high as we want
            log::warn!(
                "System hard limit for file descriptors is {} which is lower than desired {}. Consider increasing system limits.",
                rlimit.rlim_max,
                desired_soft_limit
            );

            // Set to max allowed
            rlimit.rlim_cur = rlimit.rlim_max;
        } else {
            // We can set to our desired limit
            rlimit.rlim_cur = desired_soft_limit as libc::rlim_t;
        }

        // Apply the new limits
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &rlimit) } != 0 {
            log::warn!(
                "Failed to set file descriptor limit: {}",
                std::io::Error::last_os_error()
            );
        } else {
            log::info!("Set file descriptor limit to {}", rlimit.rlim_cur);
        }
    } else {
        log::warn!(
            "Failed to get file descriptor limits: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// Run the server
pub async fn run(host: impl Into<String>, port: u16) -> Result<(), ServerError> {
    set_file_descriptor_limits();

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
    tokio::spawn({
        let cleanup_state = Arc::clone(&state); // Clone the Arc

        async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.tick().await;

            loop {
                interval.tick().await;

                match cleanup_state.lock() {
                    Ok(mut state) => {
                        state
                            .session_manager
                            .cleanup_inactive_sessions(Duration::from_secs(1800)); // 30 minutes

                        log::debug!(
                            "Cleaned up inactive sessions. Current count: {}",
                            state.session_manager.session_count()
                        );
                    }
                    Err(_) => {
                        log::error!("Failed to acquire lock for session cleanup: poisoned lock");
                    }
                }
            }
        }
    });

    // Create the listener
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(ServerError::SocketBind)?;

    // Configure the socket with more specific options
    let socket = socket2::Socket::from(listener.into_std().map_err(ServerError::SocketBind)?);
    socket.configure(|s| s.set_nonblocking(true))?;
    socket.configure(|s| s.set_reuse_address(true))?;
    socket.configure(|s| s.set_reuse_port(true))?;
    socket.configure(|s| s.set_keepalive(true))?;
    socket.configure(|s| s.set_send_buffer_size(1024 * 1024))?;
    socket.configure(|s| s.set_recv_buffer_size(1024 * 1024))?;

    // Transform the socket back into a TcpListener
    let listener = TcpListener::from_std(socket.into()).map_err(ServerError::SocketBind)?;
    log::info!("Server listening on {}", addr);

    // Create a semaphore to limit max concurrent connections
    let max_connections = std::env::var("BONKA_CONNECTION_LIMIT")
        .ok()
        .and_then(|limit| limit.parse::<usize>().ok())
        .unwrap_or_else(|| {
            // Get system limit and set to half of that, or default to a reasonable value
            let mut rlimit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlimit) } == 0 {
                // Set to half of the soft limit to leave room for other file descriptors
                (rlimit.rlim_cur as usize / 2).max(1000)
            } else {
                1000 // Default fallback
            }
        });
    let connection_limiter = Arc::new(Semaphore::new(max_connections));
    log::info!(
        "Server configured with maximum of {} concurrent connections",
        max_connections
    );

    // Backoff parameters
    let base_delay = Duration::from_millis(50);
    let max_delay = Duration::from_secs(5);
    let mut current_delay = base_delay;

    loop {
        // Acquire a permit from the semaphore before accepting a new connection
        let permit = connection_limiter
            .clone()
            .acquire_owned()
            .await
            .map_err(ServerError::SemaphoreAcquire)?;

        match listener.accept().await {
            Ok((stream, addr)) => {
                log::info!("New connection from: {}", addr);
                let client_state = Arc::clone(&state);

                // Reset backoff delay on successful connection
                current_delay = base_delay;

                // Handle each client in a separate task
                tokio::spawn(async move {
                    // The permit is moved into this task and will be dropped when the task completes
                    let _permit = permit;

                    if let Err(e) = handle_client(stream, addr.to_string(), client_state).await {
                        log::error!("Error handling client {}: {}", addr, e);
                    }
                });
            }
            Err(e) => {
                log::error!("Failed to accept connection: {}", e);
                drop(permit); // Release the permit on error

                // Apply exponential backoff with jitter
                let jitter_factor = rand::random::<f32>() * 0.5 + 0.75; // Random float between 0.75 and 1.25
                let jittered_delay = Duration::from_millis(
                    (current_delay.as_millis() as f32 * jitter_factor) as u64,
                );

                log::debug!(
                    "Backing off for {}ms before next accept attempt",
                    jittered_delay.as_millis()
                );
                tokio::time::sleep(jittered_delay).await;

                // Increase backoff delay for next failure (exponential)
                current_delay = std::cmp::min(current_delay * 2, max_delay);
            }
        }
    }
}

/// Handle a client connection
///
/// This function processes a client connection, handling requests and sending responses.
async fn handle_client(
    stream: TcpStream,
    addr: String,
    state: Arc<Mutex<ServerState>>,
) -> Result<(), ServerError> {
    // Set socket options for better performance
    stream.configure(|s| s.set_nodelay(true))?;

    // Create a session for this client
    let session_id = {
        let mut server_state = state.lock().map_err(|_| ServerError::LockingState)?;
        let session = server_state.session_manager.create_session(addr.clone());
        session.id
    };

    log::info!(
        "Created session {} for client {}",
        base62::encode(session_id),
        addr
    );

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
                    let mut server_state = state.lock().map_err(|_| ServerError::LockingState)?;
                    server_state.session_manager.touch_session(session_id);
                }

                // Process the command
                let response = process_command(request, &state).await?;

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
        let mut server_state = state.lock().map_err(|_| ServerError::LockingState)?;
        log::info!(
            "Removing session {} for client {}",
            base62::encode(session_id),
            addr
        );
        server_state.session_manager.remove_session(session_id);
    }

    log::info!("Connection closed for client {}", addr);
    Ok(())
}

/// Process a command and return a response
///
/// This function processes a command from a client request and returns a response.
async fn process_command(
    request: Request,
    state: &Arc<Mutex<ServerState>>,
) -> Result<Response, ServerError> {
    let server_state = state.lock().map_err(|_| ServerError::LockingState)?;
    let kv_store = &server_state.kv_store;

    let result = match request.command_type() {
        CommandType::CommandGet => handle_get_command(&request, kv_store),
        CommandType::CommandSet => handle_set_command(&request, kv_store),
        CommandType::CommandDelete => handle_delete_command(&request, kv_store),
        CommandType::CommandList => handle_list_command(&request, kv_store),
        CommandType::CommandExit => create_exit_response(request.id),
        _ => create_error_response(request.id, "Unknown command".to_string()),
    };

    Ok(result)
}

/// Send a response to the client
///
/// This function serializes the response using MessagePack and sends it to the client.
async fn send_response(
    framed: &mut Framed<TcpStream, LengthDelimitedCodec>,
    response: &Response,
) -> Result<(), ServerError> {
    // Serialize response using protobuf
    let encoded = response.encode_to_vec();
    // Send the response
    framed
        .send(Bytes::from(encoded))
        .await
        .map_err(ServerError::SendResponse)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv;
    use crate::proto::{self, CommandType, ResultType};
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
    fn create_proto_value(value: kv::Value) -> proto::Value {
        value.into()
    }

    // Helper function to send a command and get response
    async fn send_command(
        framed: &mut Framed<TcpStream, LengthDelimitedCodec>,
        command_type: CommandType,
        key: Option<String>,
        value: Option<proto::Value>,
    ) -> proto::Response {
        let request = proto::Request {
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
        proto::Response::decode(bytes.as_ref()).expect("Failed to deserialize response")
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

        let response = proto::Response::decode(response_bytes.as_ref())
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

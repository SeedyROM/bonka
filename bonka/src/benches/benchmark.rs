use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use futures::future::join_all;
use futures::{SinkExt, StreamExt};
use prost::Message;
use rand::{Rng, distr::Alphanumeric};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use bonka::kv::KeyValueStore;
use bonka::kv::Value;
use bonka::proto;
use bonka::proto::{CommandType, ResultType};

fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// Generate a random string of given length
fn random_string(length: usize) -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(length)
        .map(char::from)
        .collect()
}

// Benchmark the KeyValueStore directly
fn bench_kv_store(c: &mut Criterion) {
    let mut group = c.benchmark_group("KeyValueStore");
    group.measurement_time(Duration::from_secs(10));

    // Create a KV store
    let kv_store = KeyValueStore::new();

    // Benchmark setting values
    group.bench_function("set_small_string", |b| {
        b.iter(|| {
            let key = random_string(10);
            let value = Value::String(random_string(100));
            kv_store.set(key, value);
        })
    });

    group.bench_function("set_large_string", |b| {
        b.iter(|| {
            let key = random_string(10);
            let value = Value::String(random_string(10000));
            kv_store.set(key, value);
        })
    });

    // Prepare for get benchmarks
    let num_keys = 1000;
    let keys: Vec<String> = (0..num_keys).map(|_| random_string(10)).collect();

    for key in &keys {
        kv_store.set(key.clone(), Value::String(random_string(100)));
    }

    // Benchmark getting values
    group.bench_function("get_existing", |b| {
        b.iter(|| {
            let key_idx = rand::rng().random_range(0..num_keys);
            let value = kv_store.get(&keys[key_idx]);
            black_box(value);
        })
    });

    group.bench_function("get_nonexistent", |b| {
        b.iter(|| {
            let key = random_string(10);
            let value = kv_store.get(&key);
            black_box(value);
        })
    });

    // Benchmark deleting values
    group.bench_function("delete", |b| {
        // Setup: Create keys that will be deleted
        let delete_keys: Vec<String> = (0..1000)
            .map(|_| {
                let key = random_string(10);
                kv_store.set(key.clone(), Value::String(random_string(100)));
                key
            })
            .collect();

        b.iter(|| {
            let key_idx = rand::rng().random_range(0..delete_keys.len());
            kv_store.delete(&delete_keys[key_idx]);
        })
    });

    group.sample_size(10); // Use fewer samples for this expensive benchmark

    // Benchmark listing keys
    group.bench_function("list", |b| {
        b.iter(|| {
            let keys = kv_store.list();
            black_box(keys);
        })
    });

    group.finish();
}

// Benchmark the protocol serialization/deserialization
fn bench_protocol(c: &mut Criterion) {
    let mut group = c.benchmark_group("Protocol");

    // Benchmark request serialization
    group.bench_function("serialize_request", |b| {
        b.iter(|| {
            // Create a proto Value
            let proto_value = proto::Value {
                value: Some(proto::value::Value::StringValue("test-value".to_string())),
            };

            let request = proto::Request {
                id: Some(1),
                timestamp: get_timestamp(),
                command_type: CommandType::CommandSet as i32,
                key: Some("test-key".to_string()),
                value: Some(proto_value),
                metadata: Default::default(),
            };

            // Serialize using Protocol Buffers
            let buf = request.encode_to_vec();
            black_box(buf);
        })
    });

    // Benchmark response serialization
    group.bench_function("serialize_response", |b| {
        b.iter(|| {
            // Create a proto Value
            let proto_value = proto::Value {
                value: Some(proto::value::Value::StringValue("test-value".to_string())),
            };

            let response = proto::Response {
                id: Some(1),
                timestamp: get_timestamp(),
                result_type: ResultType::ResultValue as i32,
                value: Some(proto_value),
                keys: vec![],
                error: None,
                metadata: Default::default(),
            };

            // Serialize using Protocol Buffers
            let buf = response.encode_to_vec();
            black_box(buf);
        })
    });

    // Create a sample serialized request for deserialization benchmark
    let proto_value = proto::Value {
        value: Some(proto::value::Value::StringValue("test-value".to_string())),
    };

    let sample_request = proto::Request {
        id: Some(1),
        timestamp: get_timestamp(),
        command_type: CommandType::CommandGet as i32,
        key: Some("test-key".to_string()),
        value: Some(proto_value),
        metadata: Default::default(),
    };

    let request_buf = sample_request.encode_to_vec();

    // Benchmark request deserialization
    group.bench_function("deserialize_request", |b| {
        b.iter(|| {
            let request = proto::Request::decode(request_buf.as_slice()).unwrap();
            black_box(request);
        })
    });

    group.finish();
}

// Benchmark the server with concurrent clients
async fn server_benchmark() -> Result<f64, Box<dyn std::error::Error>> {
    // Start the server in a separate task
    let server_addr = "127.0.0.1:8123";

    // Use a channel to signal when the server is ready
    let (tx, rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        // Signal that we're about to start
        tx.send(()).unwrap();
        bonka::server::run("127.0.0.1".to_string(), 8123)
            .await
            .unwrap();
    });

    // Wait for server to start
    rx.await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create a number of concurrent clients
    let num_clients = 10;
    let ops_per_client = 1000;

    // Start measurement
    let start = std::time::Instant::now();

    // Spawn client tasks
    let client_futures = (0..num_clients)
        .map(|client_id| {
            tokio::spawn(async move {
                // Connect to server
                let stream = TcpStream::connect(server_addr).await.unwrap();
                let mut framed = Framed::new(stream, LengthDelimitedCodec::new());

                // Perform operations
                for i in 0..ops_per_client {
                    let key = format!("client{}-key{}", client_id, i);
                    let value = format!("value{}-{}", client_id, i);

                    // Create a Set command with protobuf
                    let proto_value = proto::Value {
                        value: Some(proto::value::Value::StringValue(value)),
                    };

                    let request = proto::Request {
                        id: Some((client_id * ops_per_client + i) as u64),
                        timestamp: get_timestamp(),
                        command_type: CommandType::CommandSet as i32,
                        key: Some(key),
                        value: Some(proto_value),
                        metadata: Default::default(),
                    };

                    // Serialize and send
                    let buf = request.encode_to_vec();
                    framed.send(Bytes::from(buf)).await.unwrap();

                    // Receive response
                    let bytes = framed.next().await.unwrap().unwrap();
                    let _response = proto::Response::decode(bytes.as_ref()).unwrap();
                }
            })
        })
        .collect::<Vec<_>>();

    // Wait for all clients to complete
    join_all(client_futures).await;

    // Calculate performance
    let elapsed = start.elapsed();
    let total_ops = num_clients * ops_per_client;
    let ops_per_second = total_ops as f64 / elapsed.as_secs_f64();

    Ok(ops_per_second)
}

fn bench_server(c: &mut Criterion) {
    let mut group = c.benchmark_group("Server");
    group.sample_size(10); // Use fewer samples for this expensive benchmark
    group.measurement_time(Duration::from_secs(30));

    group.bench_function("concurrent_clients", |b| {
        b.iter(|| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let ops_per_second = runtime.block_on(server_benchmark()).unwrap();
            black_box(ops_per_second)
        })
    });

    group.finish();
}

// Benchmark different value sizes and their impact
fn bench_value_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("ValueSizes");
    let kv_store = KeyValueStore::new();

    // Test different value sizes
    let sizes = [10, 100, 1000, 10000, 100000];

    for size in sizes {
        group.bench_with_input(BenchmarkId::new("set_string", size), &size, |b, &size| {
            b.iter(|| {
                let key = random_string(10);
                let value = Value::String(random_string(size));
                kv_store.set(key, value);
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_kv_store,
    bench_protocol,
    bench_server,
    bench_value_sizes
);
criterion_main!(benches);

#!/usr/bin/env python3
import asyncio
import argparse
import time
import random
import string
import statistics
import json
from typing import Dict, List, Any, Optional, Tuple
from dataclasses import dataclass, field
import os
import sys
import signal


# Optional imports - will show warning if not available
try:
    import matplotlib.pyplot as plt
    import numpy as np

    PLOTTING_AVAILABLE = True
except ImportError:
    PLOTTING_AVAILABLE = False
    print("Warning: matplotlib and/or numpy not available. Plotting will be disabled.")

# Import Bonka client
from bonka.client import BonkaClient


# Configuration class for benchmark parameters
@dataclass
class BenchmarkConfig:
    host: str = "localhost"
    port: int = 8379
    num_clients: int = 100
    operations_per_client: int = 1000
    value_sizes: List[int] = field(default_factory=lambda: [64, 256, 1024, 4096])
    value_size_weights: List[float] = field(
        default_factory=lambda: [0.4, 0.3, 0.2, 0.1]
    )
    max_concurrency: int = 50
    connection_pool_size: int = 20
    target_ops_per_second: Optional[int] = None
    report_interval: int = 5
    runtime: Optional[int] = None  # In seconds, None for unlimited
    operation_mix: Dict[str, float] = field(
        default_factory=lambda: {"get": 0.5, "set": 0.4, "delete": 0.05, "list": 0.05}
    )
    key_space_size: int = 10000  # Number of unique keys to use
    warmup_seconds: int = 10  # Warmup period before collecting metrics
    output_file: Optional[str] = None  # File to save results
    generate_plots: bool = False  # Generate plots
    verify_data: bool = False  # Verify data consistency


# Metrics collection for the benchmark
class BenchmarkMetrics:
    def __init__(self, config: BenchmarkConfig):
        self.config = config
        # Overall times
        self.start_time = None
        self.end_time = None
        self.warmup_end_time = None

        # Operations tracking
        self.operation_times = []
        self.operation_types = {"get": [], "set": [], "delete": [], "list": []}
        self.failures = {"get": 0, "set": 0, "delete": 0, "list": 0}
        self.successes = {"get": 0, "set": 0, "delete": 0, "list": 0}

        # Time series data for plotting
        self.time_series = []
        self.last_report_time = None
        self.last_ops_count = 0

        # Value size statistics
        self.value_size_stats = {size: [] for size in config.value_sizes}

        # Latency percentiles
        self.latency_percentiles = [50, 90, 95, 99, 99.9]

        # Data verification stats
        self.verification_failures = 0
        self.verification_successes = 0

        # Lock for thread safety
        self.lock = asyncio.Lock()

        # Flag for warmup period
        self.in_warmup = True

    async def start(self):
        """Start the benchmark metrics collection"""
        self.start_time = time.time()
        self.last_report_time = self.start_time
        self.in_warmup = True
        print(
            f"Benchmark started at {time.strftime('%H:%M:%S', time.localtime(self.start_time))}"
        )
        print(f"Warming up for {self.config.warmup_seconds} seconds...")

    async def check_warmup(self):
        """Check if warmup period is over"""
        if (
            self.in_warmup
            and time.time() - self.start_time >= self.config.warmup_seconds
        ):
            self.in_warmup = False
            self.warmup_end_time = time.time()
            # Clear metrics collected during warmup
            async with self.lock:
                self.operation_times.clear()
                for key in self.operation_types:
                    self.operation_types[key].clear()
                for key in self.failures:
                    self.failures[key] = 0
                for key in self.successes:
                    self.successes[key] = 0
                for size in self.config.value_sizes:
                    self.value_size_stats[size].clear()
                self.time_series.clear()
                self.last_report_time = time.time()
                self.last_ops_count = 0
            print(
                f"Warmup complete. Beginning measurements at {time.strftime('%H:%M:%S')}"
            )

    async def record_operation(
        self,
        op_type: str,
        duration: float,
        success: bool,
        value_size: Optional[int] = None,
    ):
        """Record an operation's metrics"""
        await self.check_warmup()

        # Skip recording during warmup
        if self.in_warmup:
            return

        async with self.lock:
            self.operation_times.append(duration)
            self.operation_types[op_type].append(duration)

            if success:
                self.successes[op_type] += 1
            else:
                self.failures[op_type] += 1

            # Record value size statistics for SET operations
            if op_type == "set" and value_size is not None:
                if value_size in self.value_size_stats:
                    self.value_size_stats[value_size].append(duration)

            # Record timestamp for this operation
            self.time_series.append((time.time(), op_type, duration))

    async def record_verification(self, success: bool):
        """Record data verification result"""
        if self.in_warmup:
            return

        async with self.lock:
            if success:
                self.verification_successes += 1
            else:
                self.verification_failures += 1

    async def finish(self):
        """Finish the benchmark and record end time"""
        self.end_time = time.time()

    def get_stats(self) -> Dict[str, Any]:
        """Get current benchmark statistics"""
        if not self.operation_times:
            return {"error": "No operations recorded yet"}

        current_time = time.time()

        # Duration excluding warmup
        if self.warmup_end_time:
            benchmark_duration = (self.end_time or current_time) - self.warmup_end_time
        else:
            benchmark_duration = (self.end_time or current_time) - self.start_time

        # Total operations
        total_ops = sum(len(times) for times in self.operation_types.values())
        total_successes = sum(self.successes.values())
        total_failures = sum(self.failures.values())

        # Only calculate throughput if we have operations
        ops_per_second = total_ops / benchmark_duration if total_ops > 0 else 0

        # Basic statistics
        stats = {
            "timestamp": current_time,
            "total_operations": total_ops,
            "successes": total_successes,
            "failures": total_failures,
            "duration_seconds": benchmark_duration,
            "operations_per_second": ops_per_second,
        }

        # Calculate operation mix
        for op_type, times in self.operation_types.items():
            stats[f"{op_type}_percentage"] = (len(times) / max(total_ops, 1)) * 100
            stats[f"{op_type}_ops"] = len(times)
            if times:
                stats[f"{op_type}_avg_ms"] = statistics.mean(times) * 1000

        # Calculate latency statistics if we have data
        if self.operation_times:
            # Sort for percentile calculations
            sorted_times = sorted(self.operation_times)

            stats.update(
                {
                    "min_latency_ms": min(sorted_times) * 1000,
                    "max_latency_ms": max(sorted_times) * 1000,
                    "avg_latency_ms": statistics.mean(sorted_times) * 1000,
                    "median_latency_ms": statistics.median(sorted_times) * 1000,
                }
            )

            # Add percentiles
            try:
                for percentile in self.latency_percentiles:
                    idx = int(len(sorted_times) * percentile / 100)
                    stats[f"p{percentile}_latency_ms"] = sorted_times[idx] * 1000
            except IndexError:
                # Not enough data points for all percentiles
                pass

            # Add standard deviation if we have enough data points
            if len(sorted_times) > 1:
                stats["std_dev_ms"] = statistics.stdev(sorted_times) * 1000

        # Recent throughput calculation
        if self.last_report_time:
            time_diff = current_time - self.last_report_time
            if time_diff > 0:
                recent_ops = total_ops - self.last_ops_count
                stats["current_throughput"] = recent_ops / time_diff

        # Update last report values
        self.last_report_time = current_time
        self.last_ops_count = total_ops

        # Value size statistics
        for size, times in self.value_size_stats.items():
            if times:
                stats[f"value_size_{size}_count"] = len(times)
                stats[f"value_size_{size}_avg_ms"] = statistics.mean(times) * 1000

        # Verification stats
        if self.config.verify_data:
            stats["verification_successes"] = self.verification_successes
            stats["verification_failures"] = self.verification_failures
            total_verifications = (
                self.verification_successes + self.verification_failures
            )
            if total_verifications > 0:
                stats["verification_success_rate"] = (
                    self.verification_successes / total_verifications
                ) * 100

        return stats

    def generate_report(self) -> Dict[str, Any]:
        """Generate a full report of benchmark results"""
        stats = self.get_stats()

        # Add more detailed information to the report
        stats["config"] = {
            "host": self.config.host,
            "port": self.config.port,
            "num_clients": self.config.num_clients,
            "operations_per_client": self.config.operations_per_client,
            "value_sizes": self.config.value_sizes,
            "max_concurrency": self.config.max_concurrency,
            "target_ops_per_second": self.config.target_ops_per_second,
            "runtime": self.config.runtime,
            "operation_mix": self.config.operation_mix,
            "key_space_size": self.config.key_space_size,
        }

        # Generate time series data in a more compact format
        if self.time_series:
            # Group by time intervals for visualization
            interval = 1  # 1 second intervals
            grouped_data = {}

            for timestamp, op_type, duration in self.time_series:
                # Round to the nearest interval
                time_bucket = int(timestamp / interval) * interval

                if time_bucket not in grouped_data:
                    grouped_data[time_bucket] = {
                        "count": 0,
                        "total_duration": 0,
                        "op_counts": {"get": 0, "set": 0, "delete": 0, "list": 0},
                    }

                grouped_data[time_bucket]["count"] += 1
                grouped_data[time_bucket]["total_duration"] += duration
                grouped_data[time_bucket]["op_counts"][op_type] += 1

            # Convert to a list of data points
            time_series_data = []
            for time_bucket, data in sorted(grouped_data.items()):
                avg_latency = (
                    (data["total_duration"] / data["count"]) if data["count"] > 0 else 0
                )
                time_series_data.append(
                    {
                        "timestamp": time_bucket,
                        "throughput": data["count"] / interval,
                        "avg_latency_ms": avg_latency * 1000,
                        "get_ops": data["op_counts"]["get"],
                        "set_ops": data["op_counts"]["set"],
                        "delete_ops": data["op_counts"]["delete"],
                        "list_ops": data["op_counts"]["list"],
                    }
                )

            stats["time_series"] = time_series_data

        return stats

    def create_plots(self, filename_prefix: str):
        """Create visualization plots from benchmark data"""
        # Check if plotting is available
        if not PLOTTING_AVAILABLE:
            print(
                "Plotting is not available. Install matplotlib and numpy to enable plots."
            )
            return

        if not self.generate_report()["time_series"]:
            print("Not enough data to generate plots")
            return

        time_series = self.generate_report()["time_series"]

        # Extract data for plotting
        timestamps = [
            point["timestamp"] - self.warmup_end_time for point in time_series
        ]
        throughput = [point["throughput"] for point in time_series]
        latency = [point["avg_latency_ms"] for point in time_series]
        get_ops = [point["get_ops"] for point in time_series]
        set_ops = [point["set_ops"] for point in time_series]
        delete_ops = [point["delete_ops"] for point in time_series]
        list_ops = [point["list_ops"] for point in time_series]

        try:
            # Create throughput over time plot
            plt.figure(figsize=(10, 6))
            plt.plot(timestamps, throughput, "b-", linewidth=2)
            plt.title("Throughput Over Time")
            plt.xlabel("Time (seconds)")
            plt.ylabel("Operations per Second")
            plt.grid(True)
            plt.savefig(f"{filename_prefix}_throughput.png")

            # Create latency over time plot
            plt.figure(figsize=(10, 6))
            plt.plot(timestamps, latency, "r-", linewidth=2)
            plt.title("Average Latency Over Time")
            plt.xlabel("Time (seconds)")
            plt.ylabel("Latency (ms)")
            plt.grid(True)
            plt.savefig(f"{filename_prefix}_latency.png")

            # Create operation mix plot
            plt.figure(figsize=(10, 6))
            plt.plot(timestamps, get_ops, "g-", label="GET")
            plt.plot(timestamps, set_ops, "b-", label="SET")
            plt.plot(timestamps, delete_ops, "r-", label="DELETE")
            plt.plot(timestamps, list_ops, "y-", label="LIST")
            plt.title("Operation Mix Over Time")
            plt.xlabel("Time (seconds)")
            plt.ylabel("Operations per Second")
            plt.legend()
            plt.grid(True)
            plt.savefig(f"{filename_prefix}_op_mix.png")

            # Create latency distribution histogram
            if self.operation_times:
                plt.figure(figsize=(10, 6))
                latencies_ms = [t * 1000 for t in self.operation_times]
                plt.hist(latencies_ms, bins=50, alpha=0.75)
                plt.title("Latency Distribution")
                plt.xlabel("Latency (ms)")
                plt.ylabel("Frequency")
                plt.grid(True)
                plt.savefig(f"{filename_prefix}_latency_dist.png")

                # Create operation type latency comparison
                plt.figure(figsize=(12, 8))
                data = []
                labels = []
                for op_type, times in self.operation_types.items():
                    if times:
                        data.append([t * 1000 for t in times])
                        labels.append(op_type.upper())

                if data:
                    plt.boxplot(data, labels=labels)
                    plt.title("Latency by Operation Type")
                    plt.ylabel("Latency (ms)")
                    plt.grid(True)
                    plt.savefig(f"{filename_prefix}_op_latency.png")

            print(f"Plots saved with prefix: {filename_prefix}")
        except Exception as e:
            print(f"Error generating plots: {e}")


# Connection pool for Bonka clients
class ConnectionPool:
    def __init__(self, host: str, port: int, max_size: int):
        self.host = host
        self.port = port
        self.max_size = max_size
        self.available = asyncio.Queue()
        self.size = 0
        self.lock = asyncio.Lock()
        self.clients_created = 0
        self.connections_borrowed = 0
        self.connections_returned = 0
        self.connection_errors = 0

    async def initialize(self, initial_size: int = 5):
        """Pre-initialize some connections in the pool"""
        print(f"Initializing connection pool with {initial_size} connections...")
        tasks = []
        for _ in range(min(initial_size, self.max_size)):
            tasks.append(self.create_connection())

        results = await asyncio.gather(*tasks, return_exceptions=True)
        successful = sum(1 for r in results if not isinstance(r, Exception))
        print(f"Successfully initialized {successful} connections")

    async def create_connection(self):
        """Create a new connection to the server"""
        client = BonkaClient(host=self.host, port=self.port)
        try:
            client.connect()
            self.clients_created += 1
            await self.available.put(client)
            self.size += 1
            return client
        except Exception as e:
            self.connection_errors += 1
            raise e

    async def get(self):
        """Get a connection from the pool or create a new one"""
        try:
            # Try to get an existing connection
            client = await asyncio.wait_for(self.available.get(), timeout=0.1)
            self.connections_borrowed += 1
            return client
        except asyncio.TimeoutError:
            # No connection immediately available
            async with self.lock:
                if self.size < self.max_size:
                    # Create a new connection
                    try:
                        client = BonkaClient(host=self.host, port=self.port)
                        client.connect()
                        self.size += 1
                        self.clients_created += 1
                        self.connections_borrowed += 1
                        return client
                    except Exception as e:
                        self.connection_errors += 1
                        raise e
                else:
                    # Wait for a connection to become available
                    client = await self.available.get()
                    self.connections_borrowed += 1
                    return client

    async def release(self, client):
        """Return a connection to the pool"""
        if client and client.connected:
            await self.available.put(client)
            self.connections_returned += 1
        else:
            # If client disconnected, reduce pool size
            async with self.lock:
                self.size -= 1

    async def close_all(self):
        """Close all connections in the pool"""
        print("Closing all connections in the pool...")
        while not self.available.empty():
            try:
                client = await self.available.get()
                if client and client.connected:
                    try:
                        client.disconnect()
                    except Exception:
                        pass
            except Exception:
                pass

        print("Connection pool statistics:")
        print(f"  Clients created: {self.clients_created}")
        print(f"  Connections borrowed: {self.connections_borrowed}")
        print(f"  Connections returned: {self.connections_returned}")
        print(f"  Connection errors: {self.connection_errors}")


# Data verification manager - tracks set values to verify consistency
class DataVerifier:
    def __init__(self):
        self.data_store = {}
        self.lock = asyncio.Lock()

    async def record_set(self, key: str, value):
        """Record a SET operation for later verification"""
        async with self.lock:
            self.data_store[key] = value

    async def verify_get(self, key: str, value) -> bool:
        """Verify a GET operation matches the expected value"""
        async with self.lock:
            if key not in self.data_store:
                return True  # We don't know what the value should be

            expected = self.data_store[key]
            return expected == value

    async def record_delete(self, key: str):
        """Record a DELETE operation"""
        async with self.lock:
            if key in self.data_store:
                del self.data_store[key]


# Generate random values of different types
class ValueGenerator:
    @staticmethod
    def random_string(length: int) -> str:
        """Generate a random string of specified length"""
        return "".join(
            random.choice(string.ascii_letters + string.digits) for _ in range(length)
        )

    @staticmethod
    def random_bytes(length: int) -> bytes:
        """Generate random bytes of specified length"""
        return bytes(random.getrandbits(8) for _ in range(length))

    @staticmethod
    def random_int() -> int:
        """Generate a random integer"""
        return random.randint(-10000000, 10000000)

    @staticmethod
    def random_float() -> float:
        """Generate a random float"""
        return random.uniform(-10000000.0, 10000000.0)

    @staticmethod
    def random_bool() -> bool:
        """Generate a random boolean"""
        return random.choice([True, False])

    @staticmethod
    def generate_value(size: int) -> Tuple[Any, str]:
        """Generate a random value of random type with approximate size"""
        value_type = random.choices(
            ["string", "bytes", "int", "float", "bool"],
            weights=[0.45, 0.45, 0.05, 0.04, 0.01],
            k=1,
        )[0]

        if value_type == "string":
            return ValueGenerator.random_string(size), "string"
        elif value_type == "bytes":
            return ValueGenerator.random_bytes(size), "bytes"
        elif value_type == "int":
            return ValueGenerator.random_int(), "int"
        elif value_type == "float":
            return ValueGenerator.random_float(), "float"
        else:  # bool
            return ValueGenerator.random_bool(), "bool"


# Client worker to perform benchmark operations
async def client_worker(
    client_id: int,
    config: BenchmarkConfig,
    metrics: BenchmarkMetrics,
    semaphore: asyncio.Semaphore,
    stop_event: asyncio.Event,
    connection_pool: ConnectionPool,
    verifier: Optional[DataVerifier] = None,
):
    """Worker function to simulate client operations"""
    operations_done = 0
    rate_limiter = None

    # Create a rate limiter if target ops/sec is specified
    if config.target_ops_per_second:
        # Calculate sleep time between operations to achieve target rate
        sleep_time = config.num_clients / config.target_ops_per_second
        rate_limiter = asyncio.create_task(asyncio.sleep(0))  # Initial no-op task

    start_time = time.time()

    try:
        while (
            (
                operations_done < config.operations_per_client
                or config.operations_per_client <= 0
            )
            and not stop_event.is_set()
            and (config.runtime is None or time.time() - start_time < config.runtime)
        ):
            if rate_limiter and not rate_limiter.done():
                await rate_limiter

            # Get a connection from the pool
            try:
                client = await connection_pool.get()
            except Exception as e:
                print(f"Client {client_id} connection error: {e}")
                # Wait before retrying
                await asyncio.sleep(1)
                continue

            try:
                # Perform an operation with concurrency control
                async with semaphore:
                    await perform_operation(
                        client=client,
                        client_id=client_id,
                        op_num=operations_done,
                        config=config,
                        metrics=metrics,
                        verifier=verifier,
                    )
            except Exception as e:
                print(f"Client {client_id} operation error: {e}")
            finally:
                # Return the connection to the pool
                await connection_pool.release(client)

            operations_done += 1

            # Set up the next rate limit wait if needed
            if config.target_ops_per_second:
                rate_limiter = asyncio.create_task(asyncio.sleep(sleep_time))

    except asyncio.CancelledError:
        pass
    except Exception as e:
        print(f"Client {client_id} encountered an error: {e}")


async def perform_operation(
    client: BonkaClient,
    client_id: int,
    op_num: int,
    config: BenchmarkConfig,
    metrics: BenchmarkMetrics,
    verifier: Optional[DataVerifier] = None,
):
    """Perform a random operation based on the operation mix"""
    # Choose an operation based on the configured mix
    op_types = list(config.operation_mix.keys())
    op_weights = list(config.operation_mix.values())
    op_choice = random.choices(op_types, weights=op_weights, k=1)[0]

    # Generate a key based on the key space size
    # Use a deterministic pattern for some keys to ensure overlap
    if random.random() < 0.8:  # 80% of operations target the "hot" keyspace
        key_id = random.randint(0, min(1000, config.key_space_size - 1))
    else:
        key_id = random.randint(0, config.key_space_size - 1)

    key = f"client{client_id % 10}-key{key_id}"
    success = True
    value_size = None

    start_time = time.time()

    try:
        if op_choice == "get":
            # GET operation
            value = client.get(key)

            # Verify data if enabled
            if config.verify_data and verifier and value is not None:
                verification_result = await verifier.verify_get(key, value)
                await metrics.record_verification(verification_result)

        elif op_choice == "set":
            # Choose a random value size based on weights
            value_size = random.choices(
                config.value_sizes, weights=config.value_size_weights, k=1
            )[0]

            # Generate a random value of random type
            value, value_type = ValueGenerator.generate_value(value_size)

            # SET operation
            client.set(key, value)

            # Record for verification if enabled
            if config.verify_data and verifier:
                await verifier.record_set(key, value)

        elif op_choice == "delete":
            # DELETE operation
            client.delete(key)

            # Record for verification if enabled
            if config.verify_data and verifier:
                await verifier.record_delete(key)

        elif op_choice == "list":
            # LIST operation
            _ = client.list()

    except Exception as e:
        print(f"Operation failed: {op_choice} {key} - {str(e)}")
        success = False

    duration = time.time() - start_time

    # Record metrics
    await metrics.record_operation(op_choice, duration, success, value_size)


# Stats reporter to periodically print benchmark status
async def stats_reporter(
    metrics: BenchmarkMetrics, interval: int, stop_event: asyncio.Event
):
    """Periodically report benchmark statistics"""
    while not stop_event.is_set():
        for _ in range(interval):
            if stop_event.is_set():
                break
            await asyncio.sleep(1)

        # Skip reporting during warmup
        if metrics.in_warmup:
            print(
                f"Warming up... ({int(time.time() - metrics.start_time)}/{metrics.config.warmup_seconds}s)"
            )
            continue

        stats = metrics.get_stats()

        # Clear screen for better visibility
        if os.name == "nt":  # Windows
            os.system("cls")
        else:  # Unix/Linux/MacOS
            os.system("clear")

        print(f"\n--- Benchmark Stats ({time.strftime('%H:%M:%S')}) ---")
        print(f"Running for: {stats['duration_seconds']:.1f}s")
        print(f"Operations: {stats['total_operations']} total")
        print(
            f"Throughput: {stats['operations_per_second']:.2f} ops/sec (avg), ", end=""
        )

        if "current_throughput" in stats:
            print(f"{stats['current_throughput']:.2f} ops/sec (current)")
        else:
            print("N/A (current)")

        print(
            f"Success rate: {stats['successes'] / max(stats['total_operations'], 1) * 100:.2f}%"
        )

        # Print latency stats if available
        if "avg_latency_ms" in stats:
            print("\nLatency (ms):")
            print(
                f"  avg={stats['avg_latency_ms']:.2f}, min={stats['min_latency_ms']:.2f}, max={stats['max_latency_ms']:.2f}"
            )
            if "p50_latency_ms" in stats:
                print(
                    f"  p50={stats['p50_latency_ms']:.2f}, p95={stats.get('p95_latency_ms', 0):.2f}, p99={stats.get('p99_latency_ms', 0):.2f}"
                )

        # Print operation mix
        print("\nOperation mix:")
        for op_type in ["get", "set", "delete", "list"]:
            if f"{op_type}_percentage" in stats:
                print(
                    f"  {op_type.upper()}: {stats[f'{op_type}_percentage']:.1f}%, ",
                    end="",
                )
                if f"{op_type}_avg_ms" in stats:
                    print(f"avg={stats[f'{op_type}_avg_ms']:.2f}ms")
                else:
                    print("N/A")

        # Print verification stats if enabled
        if "verification_successes" in stats:
            total_verifications = (
                stats["verification_successes"] + stats["verification_failures"]
            )
            if total_verifications > 0:
                print(
                    f"\nData verification: {stats['verification_success_rate']:.2f}% success rate"
                )
                if stats["verification_failures"] > 0:
                    print(
                        f"  WARNING: {stats['verification_failures']} verification failures detected!"
                    )


# Main benchmark runner
async def run_benchmark(config: BenchmarkConfig):
    """Run the benchmark with the given configuration"""
    print("\n" + "=" * 60)
    print(f"BONKA BENCHMARK - {time.strftime('%Y-%m-%d %H:%M:%S')}")
    print("=" * 60)
    print(f"Server: {config.host}:{config.port}")
    print(f"Clients: {config.num_clients}")
    print(
        f"Operations per client: {config.operations_per_client if config.operations_per_client > 0 else 'unlimited'}"
    )
    print(f"Runtime: {config.runtime}s" if config.runtime else "Runtime: unlimited")
    print(f"Concurrency: {config.max_concurrency}")
    print(f"Connection pool size: {config.connection_pool_size}")
    print(f"Value sizes: {config.value_sizes} (weights: {config.value_size_weights})")
    print(f"Operation mix: {config.operation_mix}")
    print(f"Key space size: {config.key_space_size}")
    print(f"Data verification: {'Enabled' if config.verify_data else 'Disabled'}")
    print("=" * 60)

    # Set up signal handling for graceful shutdown
    original_sigint = signal.getsignal(signal.SIGINT)
    original_sigterm = signal.getsignal(signal.SIGTERM)

    # Create metrics collector
    metrics = BenchmarkMetrics(config)

    # Create stop event
    stop_event = asyncio.Event()

    # Create connection pool
    connection_pool = ConnectionPool(
        config.host, config.port, config.connection_pool_size
    )

    # Create semaphore to limit concurrent operations
    semaphore = asyncio.Semaphore(config.max_concurrency)

    # Create data verifier if verification is enabled
    verifier = DataVerifier() if config.verify_data else None

    # Define signal handler for graceful shutdown
    def signal_handler(sig, frame):
        print("\nShutting down benchmark gracefully. Please wait...")
        stop_event.set()
        # Restore original signal handlers
        signal.signal(signal.SIGINT, original_sigint)
        signal.signal(signal.SIGTERM, original_sigterm)

    # Set signal handlers
    signal.signal(signal.SIGINT, signal_handler)
    signal.signal(signal.SIGTERM, signal_handler)

    try:
        # Initialize connection pool with some connections
        await connection_pool.initialize(min(10, config.connection_pool_size))

        # Start metrics collection
        await metrics.start()

        # Start stats reporter
        stats_task = asyncio.create_task(
            stats_reporter(metrics, config.report_interval, stop_event)
        )

        # Create client workers
        print(f"Starting {config.num_clients} client workers...")
        client_tasks = []
        for i in range(config.num_clients):
            client_tasks.append(
                asyncio.create_task(
                    client_worker(
                        client_id=i,
                        config=config,
                        metrics=metrics,
                        semaphore=semaphore,
                        stop_event=stop_event,
                        connection_pool=connection_pool,
                        verifier=verifier,
                    )
                )
            )

        # If runtime is specified, set a timer to stop the benchmark
        if config.runtime:
            print(f"Benchmark will run for {config.runtime} seconds")

            async def runtime_timer():
                await asyncio.sleep(config.runtime + config.warmup_seconds)
                if not stop_event.is_set():
                    print(
                        f"\nBenchmark reached configured runtime of {config.runtime}s. Stopping..."
                    )
                    stop_event.set()

            timer_task = asyncio.create_task(runtime_timer())

        # Wait for all clients to finish or for stop event
        done, pending = await asyncio.wait(
            client_tasks,
            return_when=asyncio.FIRST_COMPLETED
            if config.runtime is None
            else asyncio.ALL_COMPLETED,
        )

        # If we got here because of a stop event, cancel all pending tasks
        if stop_event.is_set():
            for task in pending:
                task.cancel()

            # Wait for tasks to cancel
            if pending:
                await asyncio.wait(pending, return_when=asyncio.ALL_COMPLETED)

    except Exception as e:
        print(f"Benchmark error: {e}")
    finally:
        # Signal stop to all tasks
        stop_event.set()

        # Finish metrics collection
        await metrics.finish()

        # Close all connections
        await connection_pool.close_all()

        # Cancel stats reporter if still running
        if "stats_task" in locals() and not stats_task.done():
            stats_task.cancel()
            try:
                await stats_task
            except asyncio.CancelledError:
                pass

        # Cancel runtime timer if it exists and is still running
        if "timer_task" in locals() and not timer_task.done():
            timer_task.cancel()
            try:
                await timer_task
            except asyncio.CancelledError:
                pass

        # Restore original signal handlers
        signal.signal(signal.SIGINT, original_sigint)
        signal.signal(signal.SIGTERM, original_sigterm)

        # Print final results
        print("\n" + "=" * 60)
        print("BENCHMARK RESULTS")
        print("=" * 60)

        final_stats = metrics.get_stats()

        # Print summary statistics
        print(f"Duration: {final_stats['duration_seconds']:.2f} seconds")
        print(f"Total operations: {final_stats['total_operations']}")
        print(f"Average throughput: {final_stats['operations_per_second']:.2f} ops/sec")

        if "avg_latency_ms" in final_stats:
            print("\nLatency (ms):")
            print(f"  avg={final_stats['avg_latency_ms']:.2f}")
            print(f"  min={final_stats['min_latency_ms']:.2f}")
            print(f"  max={final_stats['max_latency_ms']:.2f}")

            if "p50_latency_ms" in final_stats:
                print(f"  p50={final_stats['p50_latency_ms']:.2f}")
                print(f"  p95={final_stats.get('p95_latency_ms', 0):.2f}")
                print(f"  p99={final_stats.get('p99_latency_ms', 0):.2f}")

        print("\nOperation breakdown:")
        for op_type in ["get", "set", "delete", "list"]:
            if f"{op_type}_ops" in final_stats:
                print(
                    f"  {op_type.upper()}: {final_stats[f'{op_type}_ops']} ops", end=""
                )
                if f"{op_type}_avg_ms" in final_stats:
                    print(f", avg={final_stats[f'{op_type}_avg_ms']:.2f} ms")
                else:
                    print("")

        print(
            f"\nSuccess rate: {final_stats['successes'] / max(final_stats['total_operations'], 1) * 100:.2f}%"
        )
        print(f"Failures: {final_stats['failures']}")

        # Print verification results if enabled
        if "verification_successes" in final_stats:
            total_verifications = (
                final_stats["verification_successes"]
                + final_stats["verification_failures"]
            )
            if total_verifications > 0:
                print("\nData verification:")
                print(
                    f"  Success rate: {final_stats['verification_success_rate']:.2f}%"
                )
                print(f"  Failures: {final_stats['verification_failures']}")

        # Save results to file if specified
        if config.output_file:
            report = metrics.generate_report()
            try:
                with open(config.output_file, "w") as f:
                    json.dump(report, f, indent=2)
                print(f"\nDetailed results saved to: {config.output_file}")
            except Exception as e:
                print(f"Error saving results to file: {e}")

        # Generate plots if requested
        if config.generate_plots:
            try:
                # Use output file prefix or default
                prefix = (
                    config.output_file.rsplit(".", 1)[0]
                    if config.output_file
                    else "bonka_benchmark"
                )
                metrics.create_plots(prefix)
            except Exception as e:
                print(f"Error generating plots: {e}")

        return final_stats


def parse_arguments():
    """Parse command line arguments"""
    parser = argparse.ArgumentParser(description="Bonka Server Benchmark Tool")

    # Server connection parameters
    parser.add_argument("--host", default="localhost", help="Bonka server host")
    parser.add_argument("--port", type=int, default=8379, help="Bonka server port")

    # Workload parameters
    parser.add_argument(
        "--clients", type=int, default=100, help="Number of concurrent clients"
    )
    parser.add_argument(
        "--operations",
        type=int,
        default=1000,
        help="Operations per client (0 for unlimited)",
    )
    parser.add_argument(
        "--runtime", type=int, help="Maximum runtime in seconds (overrides operations)"
    )
    parser.add_argument(
        "--concurrency", type=int, default=50, help="Maximum concurrent operations"
    )
    parser.add_argument(
        "--pool-size", type=int, default=20, help="Connection pool size"
    )
    parser.add_argument(
        "--key-space",
        type=int,
        default=10000,
        help="Key space size (number of unique keys)",
    )

    # Value size configuration
    parser.add_argument(
        "--value-sizes",
        type=int,
        nargs="+",
        default=[64, 256, 1024, 4096],
        help="Value sizes in bytes",
    )
    parser.add_argument(
        "--value-weights",
        type=float,
        nargs="+",
        help="Weights for value sizes (must match --value-sizes)",
    )

    # Operation mix
    parser.add_argument(
        "--get-ratio", type=float, default=0.5, help="Ratio of GET operations"
    )
    parser.add_argument(
        "--set-ratio", type=float, default=0.4, help="Ratio of SET operations"
    )
    parser.add_argument(
        "--delete-ratio", type=float, default=0.05, help="Ratio of DELETE operations"
    )
    parser.add_argument(
        "--list-ratio", type=float, default=0.05, help="Ratio of LIST operations"
    )

    # Rate limiting
    parser.add_argument("--target-rate", type=int, help="Target operations per second")

    # Benchmark parameters
    parser.add_argument(
        "--warmup", type=int, default=10, help="Warmup period in seconds"
    )
    parser.add_argument(
        "--report-interval",
        type=int,
        default=5,
        help="Status report interval in seconds",
    )

    # Data verification
    parser.add_argument(
        "--verify", action="store_true", help="Enable data verification"
    )

    # Output options
    parser.add_argument("--output", help="Output file for detailed results (JSON)")
    parser.add_argument(
        "--plots", action="store_true", help="Generate performance plots"
    )

    args = parser.parse_args()

    # Validate arguments
    if args.value_weights and len(args.value_weights) != len(args.value_sizes):
        parser.error(
            "--value-weights must have the same number of elements as --value-sizes"
        )

    # Normalize operation mix
    op_mix_sum = args.get_ratio + args.set_ratio + args.delete_ratio + args.list_ratio
    if op_mix_sum <= 0:
        parser.error("Operation mix ratios must sum to a positive value")

    # Check if matplotlib is available if plots are requested
    if args.plots and not PLOTTING_AVAILABLE:
        parser.error("Cannot generate plots: matplotlib and/or numpy not available")

    return args


def main():
    """Main entry point for the benchmark"""
    args = parse_arguments()

    # Set up default value weights if not provided
    value_weights = args.value_weights
    if not value_weights:
        # Default to exponentially decreasing weights
        n = len(args.value_sizes)
        value_weights = [2 ** (n - i - 1) for i in range(n)]
        # Normalize weights to sum to 1
        total = sum(value_weights)
        value_weights = [w / total for w in value_weights]

    # Create operation mix dictionary
    operation_mix = {
        "get": args.get_ratio,
        "set": args.set_ratio,
        "delete": args.delete_ratio,
        "list": args.list_ratio,
    }

    # Normalize operation mix to sum to 1
    total_mix = sum(operation_mix.values())
    operation_mix = {k: v / total_mix for k, v in operation_mix.items()}

    # Create benchmark configuration
    config = BenchmarkConfig(
        host=args.host,
        port=args.port,
        num_clients=args.clients,
        operations_per_client=args.operations,
        value_sizes=args.value_sizes,
        value_size_weights=value_weights,
        max_concurrency=args.concurrency,
        connection_pool_size=args.pool_size,
        target_ops_per_second=args.target_rate,
        report_interval=args.report_interval,
        runtime=args.runtime,
        operation_mix=operation_mix,
        key_space_size=args.key_space,
        warmup_seconds=args.warmup,
        output_file=args.output,
        generate_plots=args.plots,
        verify_data=args.verify,
    )

    # Run the benchmark
    try:
        asyncio.run(run_benchmark(config))
    except KeyboardInterrupt:
        print("\nBenchmark interrupted by user")
    except Exception as e:
        print(f"Benchmark failed: {e}")
        import traceback

        traceback.print_exc()
        sys.exit(1)


if __name__ == "__main__":
    main()

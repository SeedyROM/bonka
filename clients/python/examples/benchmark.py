#!/usr/bin/env python3
import asyncio
import time
import random
import string
import argparse
import statistics
from dataclasses import dataclass
from typing import Dict, Any, Optional

from bonka.client import BonkaClient


# Configuration class to hold benchmark parameters
@dataclass
class BenchmarkConfig:
    host: str = "localhost"
    port: int = 8379
    num_clients: int = 100
    operations_per_client: int = 100
    value_size: int = 1000
    max_concurrency: int = 20
    target_ops_per_second: Optional[int] = None
    report_interval: int = 5


# Generate random string of specified length
def random_string(length: int) -> str:
    return "".join(
        random.choice(string.ascii_letters + string.digits) for _ in range(length)
    )


# Metrics tracking class
class BenchmarkMetrics:
    def __init__(self):
        self.start_time = time.time()
        self.end_time = None
        self.operation_times = []
        self.operation_types = {"get": [], "set": [], "delete": [], "list": []}
        self.failures = 0
        self.successes = 0
        self.operation_count = 0

    def record_operation(self, op_type: str, duration: float, success: bool):
        self.operation_times.append(duration)
        self.operation_types[op_type].append(duration)
        if success:
            self.successes += 1
        else:
            self.failures += 1
        self.operation_count += 1

    def finish(self):
        self.end_time = time.time()

    def get_stats(self) -> Dict[str, Any]:
        if not self.operation_times:
            return {"error": "No operations recorded"}

        total_duration = (
            self.end_time - self.start_time
            if self.end_time
            else time.time() - self.start_time
        )
        ops_per_second = self.operation_count / total_duration

        stats = {
            "total_operations": self.operation_count,
            "successes": self.successes,
            "failures": self.failures,
            "duration_seconds": total_duration,
            "operations_per_second": ops_per_second,
            "min_latency_ms": min(self.operation_times) * 1000
            if self.operation_times
            else 0,
            "max_latency_ms": max(self.operation_times) * 1000
            if self.operation_times
            else 0,
            "avg_latency_ms": statistics.mean(self.operation_times) * 1000
            if self.operation_times
            else 0,
            "p50_latency_ms": statistics.median(self.operation_times) * 1000
            if self.operation_times
            else 0,
        }

        if len(self.operation_times) > 1:
            try:
                stats["p95_latency_ms"] = (
                    statistics.quantiles(self.operation_times, n=20)[-1] * 1000
                )
                stats["p99_latency_ms"] = (
                    statistics.quantiles(self.operation_times, n=100)[-1] * 1000
                )
                stats["std_dev_ms"] = statistics.stdev(self.operation_times) * 1000
            except statistics.StatisticsError:
                # Handle cases with insufficient data points
                pass

        # Per operation type stats
        for op_type, times in self.operation_types.items():
            if times:
                stats[f"{op_type}_ops"] = len(times)
                stats[f"{op_type}_avg_ms"] = statistics.mean(times) * 1000

        return stats


# Run a single client worker
async def client_worker(
    client_id: int,
    config: BenchmarkConfig,
    metrics: BenchmarkMetrics,
    semaphore: asyncio.Semaphore,
    stop_event: asyncio.Event,
):
    client = BonkaClient(host=config.host, port=config.port)

    try:
        # Connect to the server
        client.connect()
        print(f"Client {client_id} connected")

        operations_done = 0
        rate_limiter = None

        # Create a rate limiter if target ops/sec is specified
        if config.target_ops_per_second:
            # Calculate sleep time between operations to achieve target rate
            sleep_time = 1.0 / (config.target_ops_per_second / config.num_clients)
            rate_limiter = asyncio.create_task(asyncio.sleep(0))  # Initial no-op task

        try:
            while (
                operations_done < config.operations_per_client
                and not stop_event.is_set()
            ):
                if rate_limiter and not rate_limiter.done():
                    await rate_limiter

                # Choose a random operation
                async with semaphore:
                    await asyncio.to_thread(
                        perform_random_operation,
                        client,
                        client_id,
                        operations_done,
                        config,
                        metrics,
                    )

                operations_done += 1

                # Set up the next rate limit wait if needed
                if config.target_ops_per_second:
                    rate_limiter = asyncio.create_task(asyncio.sleep(sleep_time))

        except Exception as e:
            print(f"Client {client_id} error during operations: {e}")
            metrics.failures += 1

    except Exception as e:
        print(f"Client {client_id} failed to connect: {e}")
        metrics.failures += 1

    finally:
        # Disconnect from server
        if client.connected:
            try:
                client.disconnect()
                print(
                    f"Client {client_id} disconnected after {operations_done} operations"
                )
            except Exception as e:
                print(f"Client {client_id} error during disconnect: {e}")


def perform_random_operation(
    client: BonkaClient,
    client_id: int,
    op_num: int,
    config: BenchmarkConfig,
    metrics: BenchmarkMetrics,
):
    # Determine operation type with weighted distribution
    # 50% get, 40% set, 5% delete, 5% list
    op_choice = random.choices(
        ["get", "set", "delete", "list"], weights=[50, 40, 5, 5], k=1
    )[0]

    key = f"client{client_id}-key{op_num % (config.operations_per_client // 4 or 1)}"
    success = True

    start_time = time.time()

    try:
        if op_choice == "get":
            # For GET, we first need to make sure a key exists
            value = client.get(key)
            if value is None and random.random() < 0.5:
                # If key doesn't exist, sometimes create it
                client.set(key, random_string(config.value_size))

        elif op_choice == "set":
            # For SET, generate random data of configured size
            value = random_string(config.value_size)
            client.set(key, value)

        elif op_choice == "delete":
            # DELETE operation
            client.delete(key)

        elif op_choice == "list":
            # LIST operation
            _keys = client.list()

    except Exception as e:
        print(f"Operation failed: {op_choice} {key} - {str(e)}")
        success = False

    duration = time.time() - start_time
    metrics.record_operation(op_choice, duration, success)


# Print periodic statistics
async def stats_reporter(
    metrics: BenchmarkMetrics, interval: int, stop_event: asyncio.Event
):
    while not stop_event.is_set():
        for _ in range(interval):
            if stop_event.is_set():
                break
            await asyncio.sleep(1)

        stats = metrics.get_stats()
        print(f"\n--- Current Benchmark Stats ({time.strftime('%H:%M:%S')}) ---")
        print(
            f"Operations: {stats['total_operations']} ({stats['operations_per_second']:.2f} ops/sec)"
        )
        print(
            f"Latency (ms): avg={stats['avg_latency_ms']:.2f}, min={stats['min_latency_ms']:.2f}, max={stats['max_latency_ms']:.2f}"
        )
        if "p95_latency_ms" in stats:
            print(
                f"p50={stats['p50_latency_ms']:.2f}, p95={stats['p95_latency_ms']:.2f}, p99={stats['p99_latency_ms']:.2f}"
            )
        print(
            f"Success rate: {stats['successes'] / (stats['total_operations'] or 1) * 100:.2f}%"
        )


async def run_benchmark(config: BenchmarkConfig):
    print(
        f"Starting benchmark with {config.num_clients} clients, {config.operations_per_client} operations each"
    )
    print(f"Server: {config.host}:{config.port}")
    print(f"Value size: {config.value_size} bytes")
    if config.target_ops_per_second:
        print(f"Target throughput: {config.target_ops_per_second} ops/sec")

    # Create metrics collector
    metrics = BenchmarkMetrics()

    # Create semaphore to limit concurrent operations
    semaphore = asyncio.Semaphore(config.max_concurrency)

    # Create stop event
    stop_event = asyncio.Event()

    # Start stats reporter
    stats_task = asyncio.create_task(
        stats_reporter(metrics, config.report_interval, stop_event)
    )

    # Create and start client workers
    client_tasks = [
        asyncio.create_task(client_worker(i, config, metrics, semaphore, stop_event))
        for i in range(config.num_clients)
    ]

    try:
        # Wait for all clients to finish
        await asyncio.gather(*client_tasks)
    except KeyboardInterrupt:
        print("\nBenchmark interrupted. Shutting down clients...")
        stop_event.set()
        # Give clients a chance to disconnect gracefully
        await asyncio.sleep(2)
    finally:
        # Signal stats reporter to stop
        stop_event.set()
        # Wait for stats reporter to finish
        await stats_task

        # Record final metrics
        metrics.finish()
        final_stats = metrics.get_stats()

        # Print final results
        print("\n" + "=" * 50)
        print("BENCHMARK RESULTS")
        print("=" * 50)
        print(f"Duration: {final_stats['duration_seconds']:.2f} seconds")
        print(
            f"Operations: {final_stats['total_operations']} total, {final_stats['operations_per_second']:.2f} ops/sec"
        )
        print(
            f"Latency (ms): avg={final_stats['avg_latency_ms']:.2f}, min={final_stats['min_latency_ms']:.2f}, max={final_stats['max_latency_ms']:.2f}"
        )
        if "p95_latency_ms" in final_stats:
            print(
                f"p50={final_stats['p50_latency_ms']:.2f}, p95={final_stats['p95_latency_ms']:.2f}, p99={final_stats['p99_latency_ms']:.2f}"
            )

        print("\nOperation breakdown:")
        for op in ["get", "set", "delete", "list"]:
            if f"{op}_ops" in final_stats:
                print(
                    f"  {op.upper()}: {final_stats[f'{op}_ops']} ops, avg={final_stats[f'{op}_avg_ms']:.2f} ms"
                )

        print(
            f"\nSuccess rate: {final_stats['successes'] / final_stats['total_operations'] * 100:.2f}%"
        )
        print(f"Failures: {final_stats['failures']}")


def main():
    parser = argparse.ArgumentParser(description="Bonka Python Client Benchmark")
    parser.add_argument("--host", default="localhost", help="Bonka server host")
    parser.add_argument("--port", type=int, default=8379, help="Bonka server port")
    parser.add_argument("--clients", type=int, default=100, help="Number of clients")
    parser.add_argument(
        "--operations", type=int, default=100, help="Operations per client"
    )
    parser.add_argument(
        "--value-size", type=int, default=1000, help="Size of values in bytes"
    )
    parser.add_argument(
        "--concurrency", type=int, default=20, help="Max concurrent operations"
    )
    parser.add_argument("--target-rate", type=int, help="Target operations per second")
    parser.add_argument(
        "--report-interval",
        type=int,
        default=5,
        help="Stats reporting interval in seconds",
    )

    args = parser.parse_args()

    config = BenchmarkConfig(
        host=args.host,
        port=args.port,
        num_clients=args.clients,
        operations_per_client=args.operations,
        value_size=args.value_size,
        max_concurrency=args.concurrency,
        target_ops_per_second=args.target_rate,
        report_interval=args.report_interval,
    )

    # Run the benchmark
    asyncio.run(run_benchmark(config))


if __name__ == "__main__":
    main()

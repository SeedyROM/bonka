#!/bin/bash

# Configuration
NUM_INSTANCES=16
HOST="localhost"
PORT=8379
CLIENTS_PER_INSTANCE=100
RUNTIME=15  # seconds

# Create a temp directory for logs
LOGDIR=$(mktemp -d)
echo "Logs will be saved to $LOGDIR"

# Function to run when script exits
cleanup() {
  echo "Cleaning up..."
  # Kill any remaining benchmark processes
  pkill -f bonka_benchmark.py || true
}

# Register cleanup function
trap cleanup EXIT

# Launch the benchmark instances
echo "Launching $NUM_INSTANCES benchmark instances..."
for i in $(seq 1 $NUM_INSTANCES); do
  # Run with different parameters to avoid key collisions
  # Each instance uses a different key prefix
  python examples/benchmark.py \
    --host $HOST \
    --port $PORT \
    --clients $CLIENTS_PER_INSTANCE \
    --runtime $RUNTIME \
    --warmup 5 \
    --key-space 1000 \
    --report-interval 10 \
    > "$LOGDIR/benchmark_$i.log" 2>&1 &
  
  PIDS[$i]=$!
  echo "Started instance $i with PID ${PIDS[$i]}"
done

echo "All benchmark instances started. Running for $RUNTIME seconds."
echo "Press Ctrl+C to terminate all instances."

# Wait for all instances to complete
wait

echo "All benchmark instances completed."
echo "Check logs in $LOGDIR for details."
#!/bin/bash

# Benchmark script for VersaTiles server response times
# Measures average response time for tile requests using the release build
# Tests a 21x21 grid of tiles (441 total requests)
# Usage: ./benchmark_server.sh [FILE] [PORT]

set -e

FILE=${1:-testdata/berlin.versatiles}
PORT=${2:-8088}

echo "═══════════════════════════════════════════════════════════"
echo "VersaTiles Server Response Time Benchmark"
echo "═══════════════════════════════════════════════════════════"
echo "File: $FILE"
echo "Port: $PORT"
echo "═══════════════════════════════════════════════════════════"
echo

# Check if file exists
if [ ! -f "$FILE" ]; then
	echo "Error: File not found: $FILE"
	exit 1
fi

# Build release version
echo "Building release version..."
cargo build --release --package versatiles 2>&1 | grep -E "(Compiling versatiles|Finished)" | tail -5
echo "✓ Build complete"
echo

# Start server
echo "Starting server..."
./target/release/versatiles serve -q "$FILE" --port $PORT &
SERVER_PID=$!

# Wait for server to be ready
sleep 2

# Check if server is running
if ! kill -0 $SERVER_PID 2>/dev/null; then
	echo "Error: Server failed to start"
	exit 1
fi

echo "✓ Server running (PID: $SERVER_PID)"
echo

# Prepare URLs to test - 21x21 grid (z=14, x=8790..8810, y=5365..5385)
URLS=()
for x in {8790..8810}; do
	for y in {5365..5385}; do
		URLS+=("http://localhost:$PORT/tiles/berlin/14/$x/$y")
	done
done

echo "Generated ${#URLS[@]} tile URLs for testing"

# Create temp file for timing results
TEMP_FILE=$(mktemp)
trap "rm -f $TEMP_FILE; kill $SERVER_PID 2>/dev/null; wait $SERVER_PID 2>/dev/null" EXIT

echo "Running benchmark..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Warmup requests
echo "Warming up (10 requests)..."
for i in {1..10}; do
	URL=${URLS[$((i % ${#URLS[@]}))]}
	curl -s -o /dev/null "$URL"
done
echo "✓ Warmup complete"
echo

# Sequential requests benchmark
echo "Running benchmark for all ${#URLS[@]} tiles..."
> "$TEMP_FILE"

i=0
for URL in "${URLS[@]}"; do
	# Use curl's time_total to measure response time in seconds
	TIME=$(curl -s -o /dev/null -w "%{time_total}\n" "$URL")
	echo "$TIME" >> "$TEMP_FILE"

	# Progress indicator
	i=$((i + 1))
	if [ $((i % 10)) -eq 0 ]; then
		echo -n "."
	fi
done
echo

NUM_REQUESTS=${#URLS[@]}

# Calculate statistics
TOTAL=$(awk '{sum+=$1} END {print sum}' "$TEMP_FILE")
AVG=$(awk '{sum+=$1} END {print sum/NR}' "$TEMP_FILE")

# Convert seconds to milliseconds for readability
AVG_MS=$(echo "$AVG * 1000" | bc)
THROUGHPUT=$(echo "scale=2; $NUM_REQUESTS / $TOTAL" | bc)

echo
echo "Results:"
echo "  Total time:    ${TOTAL}s"
echo "  Requests:      $NUM_REQUESTS"
echo "  Average:       ${AVG_MS} ms"
echo "  Throughput:    ${THROUGHPUT} req/s"
echo

echo "✓ Benchmark complete!"

exit 0

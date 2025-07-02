#!/bin/bash

echo "🚀 Starting OIF Solver Rust POC Test"
echo "====================================="

# Start the server in the background
echo "Starting server..."
cargo run &
SERVER_PID=$!

# Give the server time to start
echo "Waiting for server to start..."
sleep 5

# Test health endpoint
echo ""
echo "🩺 Testing health endpoint..."
HEALTH_RESPONSE=$(curl -s http://localhost:3000/api/v1/health)
if [[ $? -eq 0 ]]; then
    echo "✅ Health check passed"
    echo "Response: $HEALTH_RESPONSE" | jq '.'
else
    echo "❌ Health check failed"
fi

# Test API info endpoint
echo ""
echo "📋 Testing API info endpoint..."
API_INFO=$(curl -s http://localhost:3000/)
if [[ $? -eq 0 ]]; then
    echo "✅ API info endpoint passed"
    echo "Response: $API_INFO" | jq '.'
else
    echo "❌ API info endpoint failed"
fi

# Test queue status endpoint
echo ""
echo "📊 Testing queue status endpoint..."
QUEUE_RESPONSE=$(curl -s http://localhost:3000/api/v1/queue)
if [[ $? -eq 0 ]]; then
    echo "✅ Queue status endpoint passed"
    echo "Response: $QUEUE_RESPONSE" | jq '.'
else
    echo "❌ Queue status endpoint failed"
fi

# Clean up
echo ""
echo "🧹 Cleaning up..."
kill $SERVER_PID
echo "Server stopped"

echo ""
echo "✨ Test completed!" 
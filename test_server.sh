#!/bin/bash

echo "🚀 Starting OIF Solver Rust POC Test"
echo "====================================="

# Start the server in the background
echo "Starting server..."
cargo run &
SERVER_PID=$!

# Give the server time to start
echo "Waiting for server to start..."
sleep 8

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

# Test blockchain health endpoint
echo ""
echo "⛓️ Testing blockchain health endpoint..."
BLOCKCHAIN_HEALTH=$(curl -s http://localhost:3000/api/v1/health/blockchain)
if [[ $? -eq 0 ]]; then
    echo "✅ Blockchain health check passed"
    echo "Response: $BLOCKCHAIN_HEALTH" | jq '.'
else
    echo "❌ Blockchain health check failed"
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

# Test order submission with sample payload
echo ""
echo "📝 Testing order submission..."
ORDER_PAYLOAD='{
  "order": {
    "user": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
    "nonce": 123,
    "originChainId": 31337,
    "expires": 4294967295,
    "fillDeadline": 4294967295,
    "localOracle": "0x0165878A594ca255338adfa4d48449f69242Eb8F",
    "inputs": [["232173931049414487598928205764542517475099722052565410375093941968804628563", "100000000000000000000"]],
    "outputs": [{
      "remoteOracle": "0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512",
      "remoteFiller": "0x5FbDB2315678afecb367f032d93F642f64180aa3",
      "chainId": 31338,
      "token": "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0",
      "amount": "99000000000000000000",
      "recipient": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
    }]
  },
  "signature": "0x75eb67bc6bb7a6ac556912af8248336ca9125b734c9ae34682acbeb3afadd6221be45bd4df6dbc498f8ce3ed7a253cd1e705217708a5a3065388b54c960d8d711b"
}'

ORDER_RESPONSE=$(curl -s -X POST http://localhost:3000/api/v1/orders \
  -H "Content-Type: application/json" \
  -d "$ORDER_PAYLOAD")

if [[ $? -eq 0 ]]; then
    echo "✅ Order submission passed"
    echo "Response: $ORDER_RESPONSE" | jq '.'
    
    # Extract order ID for status check
    ORDER_ID=$(echo "$ORDER_RESPONSE" | jq -r '.id // empty')
    if [[ -n "$ORDER_ID" && "$ORDER_ID" != "null" ]]; then
        echo ""
        echo "🔍 Testing order status check..."
        sleep 2
        ORDER_STATUS=$(curl -s http://localhost:3000/api/v1/orders/$ORDER_ID)
        if [[ $? -eq 0 ]]; then
            echo "✅ Order status check passed"
            echo "Response: $ORDER_STATUS" | jq '.'
        else
            echo "❌ Order status check failed"
        fi
    fi
else
    echo "❌ Order submission failed"
fi

# Clean up
echo ""
echo "🧹 Cleaning up..."
kill $SERVER_PID
echo "Server stopped"

echo ""
echo "✨ Test completed!" 
#!/bin/bash

# Test Variables System with Different Environments

echo "=========================================="
echo "Testing Variables System"
echo "=========================================="
echo

# Test with development environment (default)
echo "1. Testing with DEVELOPMENT environment:"
echo "   fed --config examples/variables-example.yaml --env development status"
echo "   Expected: DEBUG_MODE=true, API_PORT=8000, POSTGRES_DB=myapp_dev, REPLICA_COUNT=1, LOG_LEVEL=debug"
echo

# Test with staging environment
echo "2. Testing with STAGING environment:"
echo "   fed --config examples/variables-example.yaml --env staging status"
echo "   Expected: DEBUG_MODE=true (default), API_PORT=8000 (default), POSTGRES_DB=myapp_staging, REPLICA_COUNT=2, LOG_LEVEL=info"
echo

# Test with production environment
echo "3. Testing with PRODUCTION environment:"
echo "   fed --config examples/variables-example.yaml --env production status"
echo "   Expected: DEBUG_MODE=false, API_PORT=8080, POSTGRES_DB=myapp_prod, REPLICA_COUNT=5, LOG_LEVEL=warn"
echo

echo "=========================================="
echo "To actually run these tests, execute:"
echo "  cargo run -- --config examples/variables-example.yaml --env development status"
echo "  cargo run -- --config examples/variables-example.yaml --env staging status"
echo "  cargo run -- --config examples/variables-example.yaml --env production status"
echo "=========================================="

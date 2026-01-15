#!/usr/bin/env bash
cd "$(dirname "$0")/.."
PROJECT_DIR=$(pwd)

set +e

NODEJS_DIR="${PROJECT_DIR}/versatiles_node"

if [ ! -d "$NODEJS_DIR" ]; then
   echo "Node.js directory not found, skipping Node.js checks"
   exit 0
fi

echo "=========================================="
echo "Node.js Checks (versatiles_node)"
echo "=========================================="

cd "$NODEJS_DIR"

# Check if node_modules exists, if not run npm install
if [ ! -d "node_modules" ]; then
   echo "Installing Node.js dependencies..."
   result=$(npm install 2>&1)
   if [ $? -ne 0 ]; then
      echo -e "$result\nERROR DURING: npm install"
      exit 1
   fi
fi

# Build the project
echo "npm run build:debug"
result=$(npm run build:debug 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: npm run build:debug"
   exit 1
fi

# TypeScript type checking
echo "npm run typecheck"
result=$(npm run typecheck 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: npm run typecheck"
   exit 1
fi

# ESLint
echo "npm run lint"
result=$(npm run lint 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: npm run lint"
   exit 1
fi

# Node.js tests
echo "npm run test"
result=$(npm run test 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: npm run test"
   exit 1
fi

# Node.js example tests
echo "npm run test:examples"
result=$(npm run test:examples 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: npm run test:examples"
   exit 1
fi

# Prettier format check
echo "npm run format:check"
result=$(npm run format:check 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: npm run format:check"
   exit 1
fi

echo "Node.js checks passed!"
exit 0

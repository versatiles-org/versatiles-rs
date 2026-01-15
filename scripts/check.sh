#!/usr/bin/env bash
cd "$(dirname "$0")/.."
PROJECT_DIR=$(pwd)

# Load GDAL environment variables
source scripts/env-gdal.sh

set +e

echo "cargo fmt"
result=$(cargo fmt -- --check 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo fmt"
   exit 1
fi

echo "cargo check"
result=$(cargo check --workspace --no-default-features --all-targets 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo check"
   exit 1
fi
echo "cargo check - server"
result=$(cargo check --workspace --no-default-features --features server --all-targets 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo check"
   exit 1
fi
echo "cargo check - cli"
result=$(cargo check --workspace --no-default-features --features cli --all-targets 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo check"
   exit 1
fi
echo "cargo check - server, cli"
result=$(cargo check --workspace --no-default-features --features server,cli --all-targets 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo check"
   exit 1
fi
echo "cargo check - all features"
result=$(cargo check --workspace --all-features --all-targets 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo check"
   exit 1
fi

echo "cargo clippy"
cd $PROJECT_DIR
result=$(cargo clippy --workspace --all-features --all-targets -- -D warnings 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo clippy"
   exit 1
fi

echo "cargo test"
cd $PROJECT_DIR
result=$(cargo test 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo test"
   exit 1
fi

echo "cargo test all features"
cd $PROJECT_DIR
result=$(cargo test --all-features 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo test all features"
   exit 1
fi

echo "cargo doc"
cd $PROJECT_DIR
result=$(RUSTDOCFLAGS="-D warnings" cargo doc --no-deps 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo doc"
   exit 1
fi

# ============================================================================
# NODE.JS CHECKS (versatiles_node)
# ============================================================================

NODEJS_DIR="${PROJECT_DIR}/versatiles_node"

if [ -d "$NODEJS_DIR" ]; then
   echo ""
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

   # Node.js tests
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

   cd "$PROJECT_DIR"
fi

echo ""
echo "=========================================="
echo "All checks passed!"
echo "=========================================="

exit 0

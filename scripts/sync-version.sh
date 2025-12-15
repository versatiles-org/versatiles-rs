#!/usr/bin/env bash
# Validates and optionally syncs version between Cargo.toml and package.json

set -e
cd "$(dirname "$0")/.."

# Extract versions
CARGO_VERSION=$(grep -m1 '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')
NPM_VERSION=$(node -p "require('./versatiles_node/package.json').version")

echo "Cargo.toml version: $CARGO_VERSION"
echo "package.json version: $NPM_VERSION"

if [ "$CARGO_VERSION" != "$NPM_VERSION" ]; then
    echo "ERROR: Version mismatch!"

    if [ "$1" == "--fix" ]; then
        echo "Syncing package.json to match Cargo.toml..."
        cd versatiles_node
        npm version "$CARGO_VERSION" --no-git-tag-version --allow-same-version
        echo "✓ Versions synchronized"
        exit 0
    else
        echo ""
        echo "Run with --fix to automatically sync package.json to Cargo.toml version"
        exit 1
    fi
fi

echo "✓ Versions are synchronized"

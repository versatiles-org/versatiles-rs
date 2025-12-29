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

        # Update main version
        npm version "$CARGO_VERSION" --no-git-tag-version --allow-same-version

        # Update optionalDependencies versions to match
        node -e "
            const fs = require('fs');
            const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'));
            if (pkg.optionalDependencies) {
                Object.keys(pkg.optionalDependencies).forEach(dep => {
                    if (dep.startsWith('@versatiles/versatiles-rs-')) {
                        pkg.optionalDependencies[dep] = '$CARGO_VERSION';
                    }
                });
                fs.writeFileSync('package.json', JSON.stringify(pkg, null, '\t') + '\n');
            }
        "

        # Update package-lock.json
        npm install --package-lock-only >/dev/null 2>&1

        echo "✓ Versions synchronized (including optionalDependencies)"
        exit 0
    else
        echo ""
        echo "Run with --fix to automatically sync package.json to Cargo.toml version"
        exit 1
    fi
fi

echo "✓ Versions are synchronized"

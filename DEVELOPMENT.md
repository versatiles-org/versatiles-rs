# Development Quick Reference

Quick reference for common development tasks in versatiles-rs.

## Check Everything

Run all checks (Rust + Node.js):

```bash
./scripts/check.sh
```

This runs:
- Rust: `cargo check`, `cargo fmt --check`, `cargo clippy`, `cargo test`, `cargo doc`
- Node.js: `npm run typecheck`, `npm run lint`, `npm run format:check`, `npm test`

## Rust Commands

### Check and Build

```bash
# Type-check workspace
cargo check --workspace --all-features --all-targets

# Format code
cargo fmt

# Check formatting
cargo fmt -- --check

# Lint with clippy
cargo clippy --workspace --all-targets -- -D warnings

# Build release
cargo build --release

# Build with GDAL support (requires GDAL installation)
source scripts/env-gdal.sh
cargo build --release --features gdal,bindgen
```

### Testing

```bash
# Run all tests
cargo test

# Run all tests with all features
cargo test --all-features

# Run specific test
cargo test test_name
```

### Documentation

```bash
# Build documentation
cargo doc --no-deps

# Build and open documentation
cargo doc --no-deps --open

# Check for documentation warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

## Node.js Commands

All Node.js commands should be run from the `versatiles_node` directory:

```bash
cd versatiles_node
```

### Check and Build

```bash
# Install dependencies
npm install

# Type-check TypeScript
npm run typecheck

# Lint with ESLint
npm run lint

# Auto-fix lint issues
npm run lint:fix

# Check formatting with Prettier
npm run format:check

# Auto-format with Prettier
npm run format

# Run all checks
npm run check

# Auto-fix everything
npm run fix
```

### Building Native Module

```bash
# Debug build (faster, for development)
npm run build:debug

# Release build (optimized, for production)
npm run build
```

### Testing

```bash
# Run all tests
npm test

# Run specific test file
npx tsx --test src/server.test.ts
```

### Examples

```bash
# Make sure you've built first
npm run build:debug

# Run examples
node examples/probe.js
node examples/convert.js
node examples/serve.js
node examples/read-tiles.js
```

## Pre-commit Hooks

### Install Lefthook

**macOS:**
```bash
brew install lefthook
```

**Linux:**
```bash
# Debian/Ubuntu
curl -1sLf 'https://dl.cloudsmith.io/public/evilmartians/lefthook/setup.deb.sh' | sudo -E bash
sudo apt install lefthook
```

**Windows:**
```powershell
scoop install lefthook
```

### Enable/Disable Hooks

```bash
# Enable hooks
lefthook install

# Disable hooks
lefthook uninstall

# Run pre-commit manually
lefthook run pre-commit

# Run pre-push manually
lefthook run pre-push
```

### Skip Hooks

```bash
# Skip all hooks for one commit
LEFTHOOK=0 git commit -m "message"

# Skip specific hook
lefthook run pre-commit --exclude rust-fmt
```

## Common Workflows

### Making Changes to Rust Code

```bash
# 1. Make your changes
# 2. Format code
cargo fmt

# 3. Run checks
./scripts/check.sh

# 4. Commit (hooks will run automatically if installed)
git add .
git commit -m "Your message"
```

### Making Changes to Node.js Code

```bash
# 1. Make your changes in versatiles_node/
# 2. Auto-fix formatting and linting
cd versatiles_node
npm run fix

# 3. Run checks
npm run check

# 4. Rebuild if you changed Rust code
npm run build:debug

# 5. Run tests
npm test

# 6. Commit from root directory
cd ..
git add .
git commit -m "Your message"
```

### Adding a New Feature

```bash
# 1. Create feature branch
git checkout -b feature/my-feature

# 2. Make changes and test
./scripts/check.sh

# 3. Commit changes
git add .
git commit -m "feat: add my feature"

# 4. Push and create PR
git push -u origin feature/my-feature
```

## Troubleshooting

### "Cannot find module '../index.js'" (Node.js)

You need to build the native module first:

```bash
cd versatiles_node
npm run build:debug
```

### GDAL Errors

Make sure GDAL is installed and environment is set:

```bash
./scripts/install-gdal.sh
source scripts/env-gdal.sh
```

### Clippy Warnings

Auto-fix what you can with `cargo fmt`, then address remaining warnings manually.

### ESLint/Prettier Errors

Auto-fix most issues:

```bash
cd versatiles_node
npm run fix
```

### Pre-commit Hook Failures

If hooks fail:

1. Check the error message
2. Run the failing command manually to debug
3. Fix the issue
4. Try committing again

Or skip hooks temporarily:

```bash
LEFTHOOK=0 git commit -m "message"
```

### Node Modules Out of Date

```bash
cd versatiles_node
rm -rf node_modules package-lock.json
npm install
```

## CI/CD

### What Runs in CI

The GitHub Actions CI workflow runs:

1. **Linux Job:**
   - Install GDAL
   - Rust checks (fmt, clippy, tests, doc)
   - Node.js checks (typecheck, lint, format, tests)
   - Code coverage

2. **Windows Job:**
   - Rust checks (fmt, tests)

### Testing CI Locally

You can approximate CI checks by running:

```bash
./scripts/check.sh
```

This covers most of what CI checks, except for coverage reporting.

## File Locations

- **Rust code:** `versatiles/`, `versatiles_*/` (various crates)
- **Node.js code:** `versatiles_node/src/`
- **Tests (Rust):** Throughout `versatiles/` and `versatiles_*/` crates
- **Tests (Node.js):** `versatiles_node/src/**/*.test.ts`
- **Examples:** `versatiles_node/examples/`
- **Scripts:** `scripts/`
- **Test data:** `testdata/`
- **Configuration:** `lefthook.yml`, `.github/workflows/ci.yml`

## Further Reading

- **Node.js Development:** [versatiles_node/CONTRIBUTING.md](versatiles_node/CONTRIBUTING.md)
- **VersaTiles Pipeline:** [versatiles_pipeline/README.md](versatiles_pipeline/README.md)
- **Configuration:** [versatiles/CONFIG.md](versatiles/CONFIG.md)
- **Official Docs:** https://docs.versatiles.org/

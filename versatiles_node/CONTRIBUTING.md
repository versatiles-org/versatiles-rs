# Contributing to @versatiles/versatiles-rs

Thank you for your interest in contributing to the VersaTiles Node.js bindings!

## Development Setup

### Prerequisites

- Node.js >= 16
- Rust toolchain (stable)
- Cargo

### Initial Setup

```bash
# Clone the repository
git clone https://github.com/versatiles-org/versatiles-rs.git
cd versatiles-rs/versatiles_node

# Install dependencies
npm install

# Build the native bindings
npm run build:debug  # For development
# or
npm run build        # For release
```

## Project Structure

```text
versatiles_node/
├── src/                # Rust source and the *.test.ts tests, see src/README.md
│   ├── tile_source.rs  # TileSource
│   ├── server.rs       # TileServer
│   ├── convert.rs      # convert()
│   ├── types/          # Option objects, TileCoord, TileJSON, …
│   └── …
├── examples/           # TypeScript examples (not published to NPM)
├── scripts/            # Build helpers, e.g. generate-vpl.ts
├── Cargo.toml          # Rust dependencies
├── package.json        # NPM package configuration; `files` decides what is published
├── build.rs            # napi-rs build script
└── README.md           # User documentation
```

## Build Process

The build process uses [napi-rs](https://napi.rs/) to create Node.js bindings from Rust code:

1. **Rust Compilation**: Cargo compiles Rust code to native libraries
2. **Binding Generation**: napi-rs generates Node.js bindings
3. **TypeScript Definitions**: Automatically generated from Rust code
4. **Platform Binaries**: Separate packages for each platform

### Development Build

```bash
npm run build:debug
```

This creates a debug build with:

- No optimizations (faster compilation)
- Debug symbols included
- Better error messages

### Release Build

```bash
npm run build
```

This creates an optimized release build with:

- Full optimizations (LTO enabled)
- Stripped symbols
- Smaller binary size

## NPM Package Contents

The `files` list in `package.json` decides what the main package contains:

- `index.js`, `index.cjs` - Generated JavaScript bindings (ESM and CommonJS)
- `index.d.ts` - TypeScript type definitions
- `vpl.js`, `vpl.d.ts` - The typed VPL builder (`@versatiles/versatiles-rs/vpl`)
- `package.json`, `README.md` - always included by npm

Everything else — Rust source, tests, examples, build files — stays out. The
native `*.node` binary is not in the main package either: each platform has its
own package, and users get the right one through `optionalDependencies`.

## Testing

### Run Examples

```bash
# Make sure you've built first
npm run build:debug

# Run individual examples
npx tsx examples/probe.ts
npx tsx examples/convert.ts
npx tsx examples/serve.ts
npx tsx examples/read-tiles.ts

# Or run all of them, as CI does
npm run test:examples
```

### Add Tests

Tests are written in TypeScript, run with [vitest](https://vitest.dev), and
located next to the code in `src/`:

```typescript
// src/example.test.ts
import { describe, expect, test } from 'vitest';
import path from 'path';
import { fileURLToPath } from 'url';
import { TileSource } from '../index.js';

const TESTDATA = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../testdata');

describe('Example', () => {
  test('should work', async () => {
    const source = await TileSource.fromPath(path.join(TESTDATA, 'berlin.mbtiles'));
    expect(source.tileJson().maxzoom).toBe(14);
  });
});
```

Run one file with `npx vitest run src/example.test.ts`.

Run tests with:

```bash
npm test
```

## Making Changes

### Adding a New Method

1. **Update Rust code** in `src/`:

   ```rust
   #[napi]
   pub async fn new_method(&self) -> Result<String> {
       Ok("result".to_string())
   }
   ```

2. **Rebuild**:

   ```bash
   npm run build:debug
   ```

3. **TypeScript definitions** are automatically generated

4. **Update documentation** in README.md

5. **Add example** in `examples/` if appropriate

### Modifying Types

1. Update type definitions in `src/types/`
2. Ensure `#[napi(object)]` or `#[napi]` attributes are correct
3. Rebuild to generate new TypeScript definitions
4. Update documentation

## Code Quality

### Running Checks

Before submitting a pull request, run:

```bash
npm run check
```

This runs, in order:

- `npm run build:debug` - Debug build of the native module and the VPL builder
- `npm run typecheck` - TypeScript type checking
- `npm run lint` - ESLint
- `npm test` - All tests
- `npm run test:examples` - Every example in `examples/`
- `npm run format:check` - Prettier format validation

### Auto-fixing Issues

To automatically fix linting and formatting issues:

```bash
npm run fix
```

This runs:

- `npm run lint:fix` - Auto-fix ESLint issues
- `npm run format` - Format all files with Prettier

### Individual Checks

Run checks individually:

```bash
npm run typecheck      # TypeScript only
npm run lint          # ESLint only
npm run format:check  # Prettier check only
npm test              # Tests only
```

## Code Style

### TypeScript

- **Strict mode**: All TypeScript code must pass strict type checking
- **Naming**: Use camelCase for variables and functions, PascalCase for classes
- **Async**: Prefer `async`/`await` over raw promises
- **Error handling**: Always handle promise rejections

### Formatting

- **Tool**: Prettier (automatic formatting)
- **Line length**: 120 characters (matches Rust)
- **Indentation**: Tabs (matches Rust)
- **Quotes**: Single quotes for strings
- **Trailing commas**: Required

Run `npm run format` to format all files automatically.

### Linting

- **Tool**: ESLint with TypeScript support
- **Config**: See `eslint.config.js`
- **Auto-fix**: Run `npm run lint:fix`

### Rust

- **Format**: Use `cargo fmt-all` to format code
- **Lint**: Use `cargo clippy` to check for issues
- **Style**: Follow standard Rust conventions

## Workflow Integration

### Repository-Wide Checks

From the repository root, run all checks (Rust + Node.js):

```bash
./scripts/check.sh
```

### Pre-commit Hooks

We recommend [Lefthook](https://github.com/evilmartians/lefthook) for automatic quality checks.

**Setup:**

```bash
brew install lefthook    # Install
lefthook install         # Enable hooks
```

**What happens:**

- **Pre-commit**: Fast checks (formatting, linting, type-checking)
- **Pre-push**: Full checks (including tests)

See [Pre-commit Hooks](#pre-commit-hooks-optional-but-recommended) section for details.

## Pre-commit Hooks (Optional but Recommended)

We recommend using [Lefthook](https://github.com/evilmartians/lefthook) to automatically run checks before commits and pushes.

### Installation

**macOS:**

```bash
brew install lefthook
```

**Linux:**

```bash
# Debian/Ubuntu
curl -1sLf 'https://dl.cloudsmith.io/public/evilmartians/lefthook/setup.deb.sh' | sudo -E bash
sudo apt install lefthook

# Or download binary from GitHub releases
```

**Windows:**

```powershell
scoop install lefthook
```

### Setup

After installing Lefthook, activate the hooks:

```bash
cd /path/to/versatiles-rs
lefthook install
```

### What Gets Checked

- **Pre-commit**: Fast checks (formatting, linting, type-checking)
- **Pre-push**: Full checks (all of the above + tests)

### Skipping Hooks

If you need to skip hooks temporarily:

```bash
# Skip pre-commit hooks
LEFTHOOK=0 git commit -m "message"

# Skip specific hook
lefthook run pre-commit --exclude rust-fmt
```

### Uninstall

```bash
lefthook uninstall
```

## Continuous Integration

All checks run automatically in GitHub Actions CI:

- Rust formatting, linting, tests
- Node.js type-checking, linting, formatting, tests
- Code coverage reporting

Pull requests must pass all CI checks before merging.

## Publishing Checklist

Before publishing a new version:

- [ ] Update version in `package.json`
- [ ] Update `CHANGELOG.md` (if exists)
- [ ] Run `cargo build --release` to verify Rust compilation
- [ ] Run `npm run build` to verify Node.js build
- [ ] Test on multiple platforms if possible
- [ ] Update documentation if API changed
- [ ] Cut the release with `scripts/release-package.sh` (see the root `RELEASING.md`)
- [ ] GitHub Actions builds the platform binaries and publishes all packages to npm

If the automated publish fails, `RELEASING.md` describes the manual fallback.

## Platform-Specific Builds

The package supports multiple platforms through separate packages:

- `@versatiles/versatiles-rs-darwin-x64` (macOS Intel)
- `@versatiles/versatiles-rs-darwin-arm64` (macOS Apple Silicon)
- `@versatiles/versatiles-rs-linux-x64-gnu` (Linux x64)
- `@versatiles/versatiles-rs-linux-arm64-gnu` (Linux ARM64)
- `@versatiles/versatiles-rs-linux-x64-musl` (Alpine Linux x64)
- `@versatiles/versatiles-rs-linux-arm64-musl` (Alpine Linux ARM64)
- `@versatiles/versatiles-rs-win32-x64-msvc` (Windows x64)
- `@versatiles/versatiles-rs-win32-arm64-msvc` (Windows ARM64)

GitHub Actions builds these automatically on release.

## Common Issues

### "Cannot find module '../index.js'"

You need to build the project first:

```bash
npm run build:debug
```

### Rust Compilation Errors

Make sure you have the latest stable Rust:

```bash
rustup update stable
```

### napi-rs Issues

Check the [napi-rs documentation](https://napi.rs/) or GitHub issues.

## Getting Help

- **Issues**: [GitHub Issues](https://github.com/versatiles-org/versatiles-rs/issues)
- **Discussions**: [GitHub Discussions](https://github.com/versatiles-org/versatiles-rs/discussions)
- **Documentation**: [VersaTiles Docs](https://docs.versatiles.org/)

## License

By contributing, you agree that your contributions will be licensed under the MIT License.

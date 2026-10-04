# NPM Package Structure

This document explains how the `@versatiles/versatiles-rs` package is structured and published.

## Package Files

### Files Included in NPM Package

When you install `@versatiles/versatiles-rs`, you get:

```text
@versatiles/versatiles-rs/
├── index.js              # ESM JavaScript bindings
├── index.cjs             # CommonJS JavaScript bindings
├── index.d.ts            # TypeScript type definitions
├── vpl.js, vpl.d.ts      # Typed VPL builder (`@versatiles/versatiles-rs/vpl`)
├── package.json          # Package metadata
└── README.md             # Documentation
```

The native binary (`*.node`) is not in this package. It comes from the
platform package npm installs alongside it, see below.

**Module Format:** The package supports both ESM (`import`) and CommonJS (`require`) through dual exports.

**Total size: ~5-15 MB** (varies by platform)

### Files Excluded from NPM Package

These files exist in the repository but are **not** published:

```text
✗ src/                    # Rust source code (~20 KB)
✗ examples/               # Example files (~30 KB)
✗ src/*.test.ts           # Test files
✗ Cargo.toml              # Rust configuration
✗ build.rs                # Build script
✗ target/                 # Build artifacts (hundreds of MB)
✗ .github/                # CI/CD configuration
✗ CONTRIBUTING.md         # Development docs
```

This is controlled by the `files` list in [package.json](./package.json), which
names exactly what is published.

## Platform-Specific Binaries

The package uses `optionalDependencies` for platform-specific binaries:

| Platform            | Package Name                                 | Binary Size |
| ------------------- | -------------------------------------------- | ----------- |
| macOS Intel         | `@versatiles/versatiles-rs-darwin-x64`       | ~5 MB       |
| macOS Apple Silicon | `@versatiles/versatiles-rs-darwin-arm64`     | ~5 MB       |
| Linux x64 (glibc)   | `@versatiles/versatiles-rs-linux-x64-gnu`    | ~8 MB       |
| Linux ARM64 (glibc) | `@versatiles/versatiles-rs-linux-arm64-gnu`  | ~8 MB       |
| Linux x64 (musl)    | `@versatiles/versatiles-rs-linux-x64-musl`   | ~8 MB       |
| Linux ARM64 (musl)  | `@versatiles/versatiles-rs-linux-arm64-musl` | ~8 MB       |
| Windows x64         | `@versatiles/versatiles-rs-win32-x64-msvc`   | ~6 MB       |
| Windows ARM64       | `@versatiles/versatiles-rs-win32-arm64-msvc` | ~6 MB       |

### How Platform Selection Works

1. User runs: `npm install @versatiles/versatiles-rs`
2. NPM detects the platform (OS + architecture)
3. NPM downloads **only** the matching platform package
4. The native binary (`.node` file) is loaded automatically

**Result:** Users only download ~5-15 MB instead of ~50+ MB for all platforms.

## Verification

### Check What Will Be Published

```bash
# Dry run to see what files will be included
npm run pack:dry

# Or use npm directly
npm pack --dry-run
```

### Inspect Installed Package

```bash
# After installation
npm ls @versatiles/versatiles-rs

# Check installed files
ls -lah node_modules/@versatiles/versatiles-rs/
```

## Publishing Process

### Manual Publishing

```bash
# 1. Ensure version is updated in package.json
# 2. Build and test
npm run build
npm test

# 3. Create package
npm pack

# 4. Inspect the tarball
tar -tzf versatiles-versatiles-rs-2.3.1.tgz

# 5. Publish (requires NPM auth)
npm publish --access public
```

### Automated Publishing (Recommended)

GitHub Actions automatically:

1. Builds binaries for all platforms
2. Creates platform-specific packages
3. Publishes to NPM on git tags

See `.github/workflows/release.yml` for configuration: a `v*` tag, or a manual
run with `publish_npm`, publishes all of them.

## Package Size Optimization

### Current Optimizations

✅ **Rust Build:**

- LTO (Link-Time Optimization) enabled
- Symbols stripped
- Release profile optimizations
- Code size optimization flags

✅ **NPM Package:**

- Excluded source files (.rs)
- Excluded examples and tests
- Excluded build artifacts
- Excluded development configs

✅ **Distribution:**

- Platform-specific packages (no bundling all platforms)
- Optional dependencies (download only what's needed)

### Size Comparison

| Package Type           | Size     | Notes                  |
| ---------------------- | -------- | ---------------------- |
| Source repository      | ~500 MB  | With build artifacts   |
| Source (no artifacts)  | ~50 KB   | Just .rs files         |
| Single platform binary | ~5-8 MB  | Optimized and stripped |
| All platform binaries  | ~50 MB   | If bundled (not done)  |
| NPM install            | ~5-15 MB | Only one platform      |

## File Size Breakdown

The main package, as `npm pack --dry-run` lists it:

```text
 31 KB  index.js                        # ESM bindings
 35 KB  index.cjs                       # CommonJS bindings
 50 KB  index.d.ts                      # TS definitions
 22 KB  vpl.js                          # VPL builder
 26 KB  vpl.d.ts                        # VPL builder types
  4 KB  package.json                    # Metadata
 18 KB  README.md                       # Documentation
────────
~190 KB Total
```

The platform package installed next to it adds the native binary, ~5-8 MB.

## Advanced: Creating Custom Builds

If you need a custom build:

```bash
# Clone repository
git clone https://github.com/versatiles-org/versatiles-rs.git
cd versatiles-rs/versatiles_node

# Install dependencies
npm install

# Build for your platform
npm run build

# Use locally
npm link

# In your project
npm link @versatiles/versatiles-rs
```

## Troubleshooting

### Package Too Large

If the package seems too large:

1. List what would be published: `npm pack --dry-run`
2. Anything unexpected there has to come from the `files` list in `package.json`
3. Check only one platform package was installed (not several)

### Missing Files

If files are missing after install:

1. Check the file is listed in the `files` field of `package.json`
2. Check platform-specific package was downloaded

### Platform Binary Not Found

If the native binary isn't loaded:

1. Verify platform is supported (check `optionalDependencies`)
2. Check network connectivity during install
3. Try: `npm install --force` to re-download
4. Check: `node_modules/@versatiles/versatiles-rs-*/` directories

## References

- [napi-rs Documentation](https://napi.rs/)
- [NPM optionalDependencies](https://docs.npmjs.com/cli/v9/configuring-npm/package-json#optionaldependencies)
- [npm pack](https://docs.npmjs.com/cli/v9/commands/npm-pack)
- [package.json `files`](https://docs.npmjs.com/cli/v9/configuring-npm/package-json#files)

# NPM Package Structure

This document explains how the `@versatiles/versatiles-rs` package is structured and published.

## Package Files

### Files Included in NPM Package

When you install `@versatiles/versatiles-rs`, you get:

```
@versatiles/versatiles-rs/
├── index.js              # Generated JavaScript bindings
├── index.d.ts            # TypeScript type definitions
├── *.node                # Native binary (platform-specific)
├── package.json          # Package metadata
└── README.md             # Documentation
```

**Total size: ~5-15 MB** (varies by platform)

### Files Excluded from NPM Package

These files exist in the repository but are **not** published:

```
✗ src/                    # Rust source code (~20 KB)
✗ examples/               # Example files (~30 KB)
✗ __test__/               # Test files
✗ Cargo.toml              # Rust configuration
✗ build.rs                # Build script
✗ target/                 # Build artifacts (hundreds of MB)
✗ .github/                # CI/CD configuration
✗ CONTRIBUTING.md         # Development docs
```

This is controlled by [.npmignore](./.npmignore).

## Platform-Specific Binaries

The package uses `optionalDependencies` for platform-specific binaries:

| Platform | Package Name | Binary Size |
|----------|--------------|-------------|
| macOS Intel | `@versatiles/versatiles-rs-darwin-x64` | ~5 MB |
| macOS Apple Silicon | `@versatiles/versatiles-rs-darwin-arm64` | ~5 MB |
| Linux x64 (glibc) | `@versatiles/versatiles-rs-linux-x64-gnu` | ~8 MB |
| Linux ARM64 (glibc) | `@versatiles/versatiles-rs-linux-arm64-gnu` | ~8 MB |
| Linux x64 (musl) | `@versatiles/versatiles-rs-linux-x64-musl` | ~8 MB |
| Linux ARM64 (musl) | `@versatiles/versatiles-rs-linux-arm64-musl` | ~8 MB |
| Windows x64 | `@versatiles/versatiles-rs-win32-x64-msvc` | ~6 MB |

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

See `.github/workflows/node-bindings.yml` for configuration.

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

| Package Type | Size | Notes |
|--------------|------|-------|
| Source repository | ~500 MB | With build artifacts |
| Source (no artifacts) | ~50 KB | Just .rs files |
| Single platform binary | ~5-8 MB | Optimized and stripped |
| All platform binaries | ~50 MB | If bundled (not done) |
| NPM install | ~5-15 MB | Only one platform |

## File Size Breakdown

Typical NPM package contents:

```
5.2 MB  versatiles.darwin-arm64.node    # Native binary
  45 KB  index.js                        # JS bindings
  12 KB  index.d.ts                      # TS definitions
   3 KB  package.json                    # Metadata
  15 KB  README.md                       # Documentation
────────
5.3 MB  Total
```

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
1. Check `.npmignore` is working: `npm pack --dry-run`
2. Verify build artifacts excluded: `ls -lah target/` (should not exist in package)
3. Check only one `.node` file included (not multiple platforms)

### Missing Files

If files are missing after install:
1. Check they're not in `.npmignore`
2. Verify `package.json` `files` field (if present)
3. Check platform-specific package was downloaded

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
- [.npmignore](https://docs.npmjs.com/cli/v9/configuring-npm/npmignore)

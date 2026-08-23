# Development Quick Reference

Quick reference for common development tasks in versatiles-rs.

## Check Everything

Run all checks (Rust + Node.js):

```bash
./scripts/check.sh
```

This runs:

- Rust: `cargo check`, `cargo fmt-check`, `cargo clippy`, `cargo test`, `cargo doc`,
  `cargo deny check` (skipped if cargo-deny is not installed)
- Node.js: `npm run typecheck`, `npm run lint`, `npm run format:check`, `npm test`
- Markdown: `markdownlint-cli2`, `prettier --check`

## Rust Commands

### Check and Build

```bash
# Type-check workspace
cargo check --workspace --all-features --all-targets

# Format code
cargo fmt-all

# Check formatting
cargo fmt-check

# Lint with clippy
cargo clippy --workspace --all-targets -- -D warnings

# Build release
cargo build --release

# Build with GDAL support (requires GDAL installation)
cargo build --release --features gdal
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
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --features gdal
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
cargo fmt-all

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

Make sure GDAL is installed via your system package manager:

```bash
./scripts/install-gdal.sh
```

### `libsqlite3-sys` conflict when bundling GDAL

This repository links GDAL dynamically against a system install, so it never hits this. You will
hit it if you depend on `versatiles` with the `gdal` feature _and_ build GDAL statically
(`gdal-sys/bundled`) — which is the usual choice for an application that ships to end users:

```text
error: failed to select a version for `libsqlite3-sys`.
    ... required by package `proj-sys v0.27.0`
package `libsqlite3-sys` links to the native library `sqlite3`, but it conflicts with a
previous package which links to `sqlite3` as well: package `libsqlite3-sys v0.38.0`
```

`libsqlite3-sys` declares `links = "sqlite3"`, so cargo permits exactly one copy per graph. Two
chains want it and their requirements do not overlap:

| Chain                                                 | Requires        |
| ----------------------------------------------------- | --------------- |
| `gdal-sys` (`bundled`) → `gdal-src` → `proj-sys` 0.27 | `>=0.28, <0.36` |
| `versatiles_container` → `r2d2_sqlite` → `rusqlite`   | `^0.38`         |

The ceiling on the `proj-sys` side predates `libsqlite3-sys` 0.36, 0.37 and 0.38; it is not a real
incompatibility, since `proj-sys` uses the crate only for linkage and for the `DEP_SQLITE3_INCLUDE`
and `DEP_SQLITE3_LIB_DIR` build-script keys, which are unchanged across those releases. The fix
belongs upstream and is proposed in [georust/proj#261](https://github.com/georust/proj/pull/261).

Until that merges, patch `proj-sys` in the **consuming application's** workspace root — `[patch]` is
only honoured there, so adding it here would not help you:

```toml
[patch.crates-io]
proj-sys = { git = "https://github.com/MichaelKreil/proj", branch = "widen-libsqlite3-sys-bound" }
```

The graph then resolves on a single `libsqlite3-sys` 0.38 shared by both chains, with no downgrade
of `rusqlite`. Remove the stanza once #261 is released. Note that a library published to crates.io
cannot carry a `[patch]` of its own, so this workaround is available to applications only.

### Clippy Warnings

Auto-fix what you can with `cargo fmt-all`, then address remaining warnings manually.

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

`.github/workflows/ci.yml` runs these jobs; `CI Success` depends on all of them
and is the status branch protection requires.

| Job                      | What it checks                                            |
| ------------------------ | --------------------------------------------------------- |
| Markdown Lint            | markdownlint over every `*.md`                            |
| Linux: Format            | `cargo fmt-check`                                         |
| Linux: Dependencies      | `cargo deny check` — advisories, licences, sources        |
| Linux: MSRV              | workspace compiles on the declared `rust-version`         |
| Linux: Docs              | `cargo doc` with `-D warnings`, gdal feature              |
| Linux: Node.js           | typecheck, lint, format, native build, Node tests         |
| Linux: Coverage          | `cargo llvm-cov`, uploaded to Codecov                     |
| Linux: Features (checks) | `cargo check` for no-default-features, cli, server        |
| Linux: Features (matrix) | clippy and tests for default, gdal, all-features          |
| Linux musl: Test         | test suite natively on musl (four release targets use it) |
| Linux ARM: Test          | test suite on aarch64                                     |
| Windows: Test            | test suite on x86_64 Windows                              |
| Windows ARM: Test        | test suite on aarch64 Windows                             |
| Windows: Node.js         | Node bindings on Windows                                  |
| macOS: GDAL              | test suite on macOS with GDAL                             |

Two things are worth knowing about the dependency job: it is the only check that
looks at _published_ vulnerabilities rather than at this repository's code, and
it can therefore fail on an unchanged repository. The workflow's weekly schedule
exists for exactly that.

Separate workflows: `codeql.yml` (CodeQL for Rust and Actions, weekly and on
`main`), `docs.yml`, and `release.yml`.

### Testing CI Locally

You can approximate CI checks by running:

```bash
./scripts/check.sh
```

That covers the Rust, Node.js and Markdown checks on your own platform. It does
not cover coverage reporting, the MSRV job, or any of the cross-platform jobs
(musl, ARM, Windows, macOS) — those need CI.

## File Locations

- **Rust code:** `versatiles/`, `versatiles_*/` (various crates)
- **Node.js code:** `versatiles_node/src/`
- **Tests (Rust):** Throughout `versatiles/` and `versatiles_*/` crates
- **Tests (Node.js):** `versatiles_node/src/**/*.test.ts`
- **Examples:** `versatiles_node/examples/`
- **Scripts:** `scripts/`
- **Test data:** `testdata/`
- **Configuration:** `lefthook.yml`, `.github/workflows/ci.yml`

## Architecture: Tile Coverage

### The Problem with Bounding Boxes

`TileBBox` — a rectangular `(x_min, y_min, x_max, y_max)` at a single zoom level — works for contiguous rectangular regions but fails for:

- Countries that straddle the 180° date line (Russia, Fiji, Kiribati)
- Island nations with scattered tiles
- Any source whose coverage is not a single rectangle

### `TileQuadtree`

`TileQuadtree` represents an **arbitrary set of tiles** at a single zoom level using a quadtree. Each node is one of:

- `Empty`: no tiles covered in this subtree
- `Full`: all tiles covered in this subtree
- `Partial`: children are [NW, NE, SW, SE], each also a node

Uniform regions collapse to a single node regardless of size — a fully covered continent at zoom 14 is one `Full` node. Non-rectangular or scattered coverage is represented exactly without approximation.

Key properties:

- **Space-filling**: memory proportional to the number of coverage _boundaries_, not tiles
- **Serializable**: compact 2-bit-per-node prefix encoding
- **Set operations**: `union`, `intersection`, `difference` short-circuit on `Full`/`Empty` nodes

```rust
// Build from a bounding box or geographic coordinates
let qt = TileQuadtree::from_bbox(&some_bbox);
let qt = TileQuadtree::from_geo(zoom, &geo_bbox)?;

// Insert individual tiles or bboxes
qt.insert_coord(&coord)?;
qt.insert_bbox(&bbox)?;

// Set operations
let union = a.union(&b)?;
let inter = a.intersection(&b)?;
```

### `TileCover`

`TileCover` is an enum that represents tile coverage at **one zoom level**, wrapping either a rectangle or a quadtree:

```rust
pub enum TileCover {
    Bbox(TileBBox),    // rectangular, fast
    Tree(TileQuadtree), // arbitrary shape, exact
}
```

Starts as `Bbox` for all constructors that produce rectangular coverage. Automatically upgrades to `Tree` when a non-rectangular operation is requested (`remove_coord`, `remove_bbox`, `intersect_bbox`, `difference`).

### `TilePyramid`

`TilePyramid` holds one `TileCover` per zoom level (0 through `MAX_ZOOM_LEVEL = 30`). It is the **primary type for tile coverage tracking** and is accessed via `TileSourceMetadata::tile_pyramid()`.

```rust
let mut pyramid = TilePyramid::new_empty();
pyramid.insert_bbox(&bbox)?;
pyramid.intersect_geo_bbox(&geo_bbox)?;

let min_zoom = pyramid.level_min(); // Option<u8>
let max_zoom = pyramid.level_max(); // Option<u8>
let geo      = pyramid.geo_bbox();  // Option<GeoBBox>
```

### `PyramidInfo` Trait

`TilePyramid` implements the `PyramidInfo` trait, which exposes the metadata fields needed by `TileJSON::update_from_pyramid`:

```rust
pub trait PyramidInfo {
    fn get_geo_bbox(&self) -> Option<GeoBBox>;
    fn get_zoom_min(&self) -> Option<u8>;
    fn get_zoom_max(&self) -> Option<u8>;
}
```

### What Uses What

| Type           | Used for                                                                        |
| -------------- | ------------------------------------------------------------------------------- |
| `TileQuadtree` | Exact coverage at one zoom level — arbitrary tile shapes, set operations        |
| `TileCover`    | Coverage at one zoom level — rectangular (fast) or quadtree (exact)             |
| `TilePyramid`  | Multi-zoom coverage tracking — which tiles exist across all zoom levels         |
| `TileBBox`     | Rectangular geometry — image dimensions, request shapes, container block layout |

`TileBBox` is kept for anything that is inherently rectangular: requesting a range of tiles from a container, describing image dimensions, wire format block indices. `TilePyramid` is used wherever the question is "does this tile exist in this data source?"

### Why Geographic Intersection Needs No Quadtree

`TilePyramid::intersect_geo_bbox` intersects every zoom level against a plain `TileBBox`, never a `TileQuadtree`. That is not an approximation: a `GeoBBox` is a rectangle, and both Web Mercator axes are monotonic in longitude and latitude, so the tiles covering a geographic rectangle are themselves always a rectangle. A quadtree could not describe that set more precisely, and building one at high zoom would cost O(perimeter_tiles) nodes — gigabytes of RAM around zoom 17 and above.

Quadtree coverage earns its keep where the shape genuinely is not rectangular — unions of separate regions, or coverage read back from a container — which is what `TileCover::Tree` is for.

## Further Reading

- **Writing rustdoc:** [DOCSTYLE.md](DOCSTYLE.md)
- **Writing VPL operation docs:** [versatiles_pipeline/DOCSTYLE.md](versatiles_pipeline/DOCSTYLE.md)
- **Node.js Development:** [versatiles_node/CONTRIBUTING.md](versatiles_node/CONTRIBUTING.md)
- **VersaTiles Pipeline:** [versatiles_pipeline/README.md](versatiles_pipeline/README.md)
- **Configuration:** [versatiles/CONFIG.md](versatiles/CONFIG.md)
- **Official Docs:** <https://docs.versatiles.org/>

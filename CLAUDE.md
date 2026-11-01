# versatiles-rs

Rust workspace behind the `versatiles` CLI, which converts, checks and serves map tiles (`.versatiles`, MBTiles, PMTiles, tar), including through VPL pipelines. `versatiles_node` publishes the same code to npm with NAPI-RS.

## Crates

Dependencies point upward: each crate uses only crates above it.

| Crate                  | Role                                                                              |
| ---------------------- | --------------------------------------------------------------------------------- |
| `versatiles_derive`    | Proc macros, including the `VPLDecode` derive that generates the VPL docs         |
| `versatiles_core`      | Tile types, I/O (file, HTTP, SFTP), compression, JSON. Must stay wasm32-buildable |
| `versatiles_geometry`  | Vector tiles, GeoJSON and geometry. `mlt` feature for MapLibre Tiles              |
| `versatiles_image`     | Raster formats and image traits                                                   |
| `versatiles_container` | Container readers and writers, `TilesRuntime`, conversion                         |
| `versatiles_pipeline`  | VPL parser and operations. `gdal` feature for GDAL-backed sources                 |
| `versatiles`           | CLI and server (`cli`, `server`, `sftp`, `mlt` are its default features)          |
| `versatiles_node`      | npm bindings, not published to crates.io                                          |

`fuzz/` is a separate workspace built on nightly: `cargo +nightly fuzz run <target>`.

## Git in this repo

These rules override the shared commit rules:

- Commit finished, checked work on the checked-out branch without asking, one logical change per commit. This replaces recommending a commit message.
- A commit that completes an issue ends with `, close #N`. One that only advances it ends with `(#N)`. Example: `feat(pipeline): add dem_hillshade, close #289`.
- Never push or run any other git command that writes to a remote, even if a task says "deploy" or "publish". Stop after the commit and say it's ready to push.
- Never create branches or worktrees unless the user explicitly asks. The branch and push steps in `DEVELOPMENT.md` are for human contributors.
- Attribution is also banned in PRs, issues and comments. Older commits with trailers don't change that.
- A request to post one thing covers only that thing, not a follow-up comment. Editing or deleting something already published also needs asking.
- `scripts/release-package.sh` pushes `dev`, fast-forwards `main` and pushes the tag itself. Run it only when asked.

## Commands

Quick checks during work:

```bash
cargo fmt-all                  # never bare `cargo fmt`, see Pitfalls
cargo test -p <crate> <filter>
cargo clippy --workspace --all-targets -- -D warnings
```

Full local check, needs GDAL (`./scripts/install-gdal.sh`): `./scripts/check.sh`. It runs `check-rust.sh`, `check-node.sh` and `check-markdown.sh`, and is slow.

What CI runs (`.github/workflows/ci.yml`):

```bash
cargo fmt-check
python3 scripts/check-release-profile.py
./scripts/check-crate-licenses.sh
cargo deny check
cargo check --workspace --lib --bins       # with RUSTUP_TOOLCHAIN set to the MSRV
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items --features gdal,versatiles_container/test
cargo hack clippy --workspace --each-feature --exclude-features gdal,bindgen --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace                     # also with --features gdal and with --all-features
cargo check --workspace --no-default-features   # Windows job
cargo llvm-cov test --workspace --all-features --tests --lcov --output-path ./lcov.info -- --skip e2e_
node scripts/check-reduce-cases.mjs
versatiles_node/node_modules/.bin/prettier --check "**/*.md"
npx --yes markdownlint-cli2 "**/*.md" "#.**" "#*_PLAN.md" "#versatiles_node/node_modules" "#target"   # CI uses the action
```

In `versatiles_node/`, CI runs these in order. `npm run check` is the local equivalent with a debug build:

```bash
npm install
npm run build        # build:debug on Windows
npm run typecheck
npm run lint
npm run format:check
npm run test
npm run test:examples
```

Regenerate after a change:

```bash
./scripts/build-docs-readme.sh   # versatiles_pipeline/README.md, versatiles/CONFIG.md; never edit these by hand
UPDATE_METADATA_SNAPSHOT=1 cargo test -p versatiles_pipeline --features codegen,gdal --test metadata_surface
```

## Design rules

- A container writer never silently degrades its output. If it can't keep a format guarantee, it returns an error naming the options that would permit the result. Don't warn and continue, and don't pick a remedy on the user's behalf, because an expensive remedy is the user's cost to choose. PMTiles is the model: `allow_unclustered=true` and `reorder=true` are both opt-in.
- Writer-specific settings are writer options: `--writer-option key=value` in the CLI, `writerOptions` in Node, and the `writer_option` builder in Rust. They travel on `TilesRuntime`, since it's the only argument writers receive. `TilesConverterParameters` never reaches the writer.
- SSH private keys are files on disk. New SFTP auth options take a path, never key bytes, and there is no in-memory key handling or zeroize-style hardening. Auth stays in `open_session` in `versatiles_core/src/io/sftp_utils.rs`.
- The public contract is the container formats, `convert`, `probe`, `serve` and their documented options, and the documented tile and TileJSON URLs. Breaking any of these needs a major release.
- VPL and the Rust API may change in a minor release, but never silently: list the change under "Upgrading" in `.github/release-notes/vX.Y.Z.md`, and make a removed parameter an error that names its replacement. `RELEASING.md` has the full policy.
- VPL metadata is consumed by `versatiles-studio`, so a change to `rust_type` or a rename can break its forms without a compile error. `metadata_surface.snap` shows such changes, so review its diff.
- Lint suppressions use `#[expect(..., reason = "...")]`. `Cargo.toml` lists the three cases that must stay `#[allow(..., reason = "...")]`.
- Every public item needs a doc comment. Follow `DOCSTYLE.md`, and `versatiles_pipeline/DOCSTYLE.md` for VPL text, which `tests/docs_style.rs` partly enforces.

## Pitfalls

- Bare `cargo fmt` gives wrong results: `imports_granularity` and `group_imports` only work as `--config` flags, so use the `fmt-all` and `fmt-check` aliases from `.cargo/config.toml`.
- `check.sh` changes files: `cargo fmt-all` and `markdownlint --fix` both write in place, so look at the diff afterwards.
- `check.sh` isn't full CI. It tests only `--all-features`, skips `cargo-hack` and `cargo-deny` if they're missing, and doesn't cover the MSRV or other platforms.
- Workspace builds hide feature bugs, because `versatiles_node` always enables `versatiles_container/sftp`. Use `cargo hack` or `-p <crate>` to see a crate as crates.io users get it.
- Keep the `cargo doc` flags in `ci.yml` and `check-rust.sh` identical. A bare backtick mention of a removed type is invisible to rustdoc, so use intra-doc links.
- `versatiles/Cargo.toml` copies the workspace release profile for `cargo install`. Change both or `check-release-profile.py` fails.
- `overflow-checks` stays on in release for `versatiles_core`, `versatiles_container` and `versatiles_geometry` on purpose, since they parse untrusted bytes. `Cargo.toml` has the reasoning.
- A new published crate needs `LICENSE` as a symlink to the root file.
- Checking the MSRV needs `RUSTUP_TOOLCHAIN`, because `rust-toolchain.toml` outranks `rustup default`.
- `versatiles_core` builds for wasm32, but no CI job checks that. Keep networking behind the `sftp` feature, including tokio's.
- Prettier and markdownlint cover this file too, like almost every `*.md` in the repo. `**.md` is in `paths-ignore`, so a Markdown-only push doesn't trigger CI.
- `versatiles_node/index.*` and `vpl.*` are generated and gitignored. Run `npm run build:debug` after Rust changes before running Node tests. "Cannot find module '../index.js'" means no build yet.
- `.gitignore` covers `*.txt` and `*.versatiles`. `testdata/berlin.versatiles` and `testdata/places.txt` may exist locally but aren't tracked, so committed tests can't rely on them.
- The `drawn` values in `versatiles_pipeline/tests/reduce_cases/` are handwritten. The Rust tests trust them and only `check-reduce-cases.mjs` compares them with MapLibre, so run it when editing cases.
- Tests named `e2e_*` are skipped in coverage.
- `cargo deny` can fail with no repo change when a new advisory appears. Exceptions and their reasons go in `deny.toml`.
- CI's cache store sits at GitHub's 10 GB ceiling and only `main` writes to it. A new cache entry evicts another one, so read "Cache policy" in `ci.yml` before adding one.
- Versions are shared across manifests, `Cargo.lock` and `package.json`. The release script sets them, and `./scripts/sync-version.sh` validates them. Releases are cut from `dev`.

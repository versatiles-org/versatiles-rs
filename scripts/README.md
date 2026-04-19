# Scripts

Development, testing, and CI/CD automation scripts for the VersaTiles Rust workspace.

## Quick reference

| Script                                                         | Purpose                                                                        |
|----------------------------------------------------------------|--------------------------------------------------------------------------------|
| **[`check.sh`](#checksh)**                                     | **Run all quality checks (Rust + Node.js + Markdown) — run before committing** |
| **[`build-release-with-gdal.sh`](#build-release-with-gdalsh)** | **Build release binary with GDAL support**                                     |
| **[`release-package.sh`](#release-packagesh)**                 | **Create and tag a versioned release**                                         |

---

## Build

### `build-debug-with-gdal.sh`

Build the debug binary with GDAL support enabled.

```sh
./scripts/build-debug-with-gdal.sh
```

Requires GDAL development libraries — install first with [`install-gdal.sh`](#install-gdalsh).

---

### `build-release-with-gdal.sh`

Build the release binary with GDAL support. Optionally installs it to `/usr/local/bin`.

```sh
./scripts/build-release-with-gdal.sh [--install]
```

Requires GDAL development libraries — install first with [`install-gdal.sh`](#install-gdalsh).

---

### `build-docker-gdal.sh`

Build the GDAL-enabled Docker image (`versatiles-gdal`) from `docker/gdal-debian.Dockerfile`.

```sh
./scripts/build-docker-gdal.sh
```

---

### `build-docker-images.sh`

Build and smoke-test Docker images for all supported Linux base images (debian, alpine, scratch) on `linux/amd64`.

```sh
./scripts/build-docker-images.sh
```

Runs `selftest-versatiles.sh` inside each image after building. Requires Docker Buildx.

---

### `build-docs.sh`

Generate Rust API documentation with `cargo doc`.

```sh
./scripts/build-docs.sh
```

Clears `./doc/` and rebuilds HTML docs for all workspace crates (excluding dependencies).

---

### `build-docs-readme.sh`

Regenerate the pipeline and config reference READMEs from the built binary.

```sh
./scripts/build-docs-readme.sh
```

Builds a debug binary with GDAL and overwrites:

- `versatiles_pipeline/README.md` — from `versatiles help --raw pipeline`
- `versatiles/CONFIG.md` — from `versatiles help --raw config`

---

## Check / Quality

### `check.sh`

Run all quality checks: Rust, Node.js, and Markdown. Run this before committing or opening a pull request.

```sh
./scripts/check.sh
```

Delegates to `check-rust.sh`, `check-node.sh`, and `check-markdown.sh` in order.

---

### `check-rust.sh`

Run all Rust quality checks across the workspace.

```sh
./scripts/check-rust.sh
```

Steps: `rustfmt`, `cargo check` (multiple feature combinations), `clippy -D warnings`, tests with all features, doc build with `-D warnings`.

---

### `check-node.sh`

Run all Node.js quality checks for the `versatiles_node` package.

```sh
./scripts/check-node.sh
```

Steps: `npm install` (if needed), debug build, TypeScript typecheck, ESLint, Vitest tests, example tests, Prettier format check. Skips if `versatiles_node/` does not exist.

---

### `check-markdown.sh`

Lint all Markdown files in the repository with `markdownlint-cli2`.

```sh
./scripts/check-markdown.sh
```

---

## Test

### `test-unix.sh`

Developer test script: format, lint, and test the Rust workspace on Unix.

```sh
./scripts/test-unix.sh [extra-cargo-args]
```

Runs `rustfmt`, `clippy` (binary + lib, multiple feature combinations), and `cargo test` (bins, lib, doc tests).

---

### `test-windows.ps1`

Run Rust quality checks on Windows (PowerShell equivalent of `test-unix.sh`).

```powershell
./scripts/test-windows.ps1
```

---

### `test-coverage.sh`

Generate code coverage reports with `cargo llvm-cov`.

```sh
./scripts/test-coverage.sh [extra-args]
```

Outputs `lcov.info` at the repo root. Skips e2e tests (`e2e_` prefix).

---

### `test-timing.sh`

Measure and analyse per-test runtimes to identify slow tests.

```sh
./scripts/test-timing.sh [cargo-test-args]
./scripts/test-timing.sh --package versatiles_pipeline
./scripts/test-timing.sh -- my_specific_test
```

Requires the nightly toolchain (`rustup toolchain install nightly`). Outputs a ranked list of the 30 slowest tests and a per-module summary.

---

### `perf-benchmarks.sh`

Run all unit tests with per-test timing via libtest's `--report-time` flag.

```sh
./scripts/perf-benchmarks.sh
```

Requires the nightly toolchain. For a richer analysis with ranking and module summaries, use `test-timing.sh` instead.

---

### `bench-lossless.sh`

Run lossless compression benchmarks for WebP and PNG image formats.

```sh
./scripts/bench-lossless.sh
```

Executes example binaries from the `versatiles_image` crate.

---

### `selftest-versatiles.sh`

Smoke-test the versatiles binary with a convert and serve command.

```sh
./scripts/selftest-versatiles.sh [path-to-binary]
```

Defaults to `versatiles` on `PATH`. Used inside Docker image builds to verify the binary works in the target environment.

---

## Install

### `install-gdal.sh`

Install GDAL development libraries via the system package manager.

```sh
./scripts/install-gdal.sh
```

Supports Debian/Ubuntu (`apt`), Alpine (`apk`), and macOS (`brew`). Required before building with the `gdal` feature.

---

### `install-unix.sh`

Install the VersaTiles binary on Unix by downloading the correct precompiled release binary.

```sh
curl -Ls "https://github.com/versatiles-org/versatiles-rs/releases/latest/download/install-unix.sh" | sudo sh
```

---

### `install-windows.ps1`

Install the VersaTiles binary on Windows by downloading the correct precompiled release binary.

```powershell
./scripts/install-windows.ps1
```

---

## Release & Maintenance

### `release-package.sh`

Interactively create a versioned release by bumping the version, tagging, and committing.

```sh
./scripts/release-package.sh              # interactive menu
./scripts/release-package.sh patch        # patch / minor / major / alpha / beta / rc / dev
```

After running, push with `git push origin main --follow-tags` to trigger the CI release workflow.

---

### `sync-version.sh`

Validate and optionally sync the version between `Cargo.toml` and `package.json`.

```sh
./scripts/sync-version.sh
```

---

### `upgrade-deps.sh`

Update Rust dependencies to their latest compatible versions.

```sh
./scripts/upgrade-deps.sh
```

---

### `audit-unused-deps.sh`

Find unused dependencies in the workspace with `cargo machete`.

```sh
./scripts/audit-unused-deps.sh
```

---

## Analysis & Profiling

### `analyze-binary-size.sh`

Analyse the size of the release binary, breaking down contributions by crate and dependency.

```sh
./scripts/analyze-binary-size.sh
```

---

### `doc-coverage-report.sh`

Generate a documentation coverage report for all public API items.

```sh
./scripts/doc-coverage-report.sh
```

---

### `profile-macos.sh`

Profile the versatiles binary on macOS using Instruments (CPU Profiler).

```sh
./scripts/profile-macos.sh
```

Requires Xcode Instruments. Edit the script to change the profiling target or arguments.

---

### `stress-ddos.sh`

Load-test a local tile server with parallel HTTP requests.

```sh
./scripts/stress-ddos.sh
```

Sends 300 tile requests (10 in parallel) to `localhost:8080` and reports total elapsed time. Requires GNU parallel and a running server.

---

## CI / Workflow

### `workflow-create-release.sh`

CI script: create a draft GitHub release for the latest version tag.

Fetches the two most recent version tags, assembles a changelog from commits between them, and creates a draft pre-release. Intended for use inside GitHub Actions.

---

### `workflow-pack-upload.sh`

CI script: package a compiled binary as `.tar.gz` and upload it to a GitHub release.

```sh
./scripts/workflow-pack-upload.sh <folder> <filename-stem> <tag>
```

---

### `workflow-pack-upload.ps1`

PowerShell equivalent of `workflow-pack-upload.sh` for Windows CI runners.

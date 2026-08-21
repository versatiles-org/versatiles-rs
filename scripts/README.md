# Scripts

Development, testing, and CI/CD automation scripts for the VersaTiles Rust workspace.

## Scripts

### Build

| Script | Purpose |
| --- | --- |
| `build-debug-with-gdal.sh` | Build the debug binary with GDAL support enabled |
| [`build-release-with-gdal.sh`](#build-release-with-gdalsh) | Build the release binary with GDAL support. Optionally installs it to `/usr/local/bin` |
| `build-docker-gdal.sh` | Build the GDAL-enabled Docker image (`versatiles-gdal`) from `docker/gdal-debian.Dockerfile` |
| `build-docker-images.sh` | Build and smoke-test Docker images for all supported Linux base images (debian, alpine, scratch) on `linux/amd64` |
| `build-docs.sh` | Generate Rust API documentation with `cargo doc` |
| `build-docs-readme.sh` | Regenerate the pipeline and config reference READMEs from the built binary |

### Check / Quality

| Script | Purpose |
| --- | --- |
| `check.sh` | Run all quality checks: Rust, Node.js, and Markdown. Run this before committing or opening a pull request |
| `check-rust.sh` | Run all Rust quality checks across the workspace |
| `check-node.sh` | Run all Node.js quality checks for the `versatiles_node` package |
| `check-markdown.sh` | Lint all Markdown files in the repository with `markdownlint-cli2` |
| `format.sh` | Auto-format the codebase in place — the write-mode counterpart to `check.sh` |

### Test

| Script | Purpose |
| --- | --- |
| [`test-unix.sh`](#test-unixsh) | Developer test script: format, lint, and test the Rust workspace on Unix |
| `test-windows.ps1` | Run Rust quality checks on Windows (PowerShell equivalent of `test-unix.sh`) |
| [`test-coverage.sh`](#test-coveragesh) | Generate code coverage reports with `cargo llvm-cov` |
| [`test-timing.sh`](#test-timingsh) | Measure and analyse per-test runtimes to identify slow tests |
| `perf-benchmarks.sh` | Run all unit tests with per-test timing via libtest's `--report-time` flag |
| `bench-lossless.sh` | Run lossless compression benchmarks for WebP and PNG image formats |
| [`selftest-versatiles.sh`](#selftest-versatilessh) | Smoke-test the versatiles binary with a convert and serve command |

### Install

| Script | Purpose |
| --- | --- |
| `install-gdal.sh` | Install GDAL development libraries via the system package manager |
| [`install-unix.sh`](#install-unixsh) | Install the VersaTiles binary on Unix by downloading the correct precompiled release binary |
| `install-windows.ps1` | Install the VersaTiles binary on Windows by downloading the correct precompiled release binary |

### Release & Maintenance

| Script | Purpose |
| --- | --- |
| [`release-package.sh`](#release-packagesh) | Interactively create a versioned release by bumping the version, tagging, and committing |
| `sync-version.sh` | Validate and optionally sync the version between `Cargo.toml` and `package.json` |
| `upgrade-deps.sh` | Update Rust dependencies to their latest compatible versions |
| `audit-unused-deps.sh` | Find unused dependencies in the workspace with `cargo machete` |
| [`clean-target.sh`](#clean-targetsh) | Reclaim disk space in `target/` without discarding what you are still building. Cargo never garbage-collects `target/`: every configuration ever built stays there — six feature sets across the check matrix, dev and test profiles, a tree per `--target` release build, a complete second tree under `llvm-cov-target/`, and every dependency version that predates a `cargo update`. One `cargo test --no-run` is about 3.6 GB, so the total is that figure times however many distinct builds have piled up |

### Analysis & Profiling

| Script | Purpose |
| --- | --- |
| `analyze-binary-size.sh` | Analyse the size of the release binary, breaking down contributions by crate and dependency |
| `doc-coverage-report.sh` | Generate a documentation coverage report for all public API items |
| `profile-macos.sh` | Profile the versatiles binary on macOS using Instruments (CPU Profiler) |
| `stress-ddos.sh` | Load-test a local tile server with parallel HTTP requests |

### CI / Workflow

| Script | Purpose |
| --- | --- |
| `workflow-create-release.sh` | Fetches the last two version tags, assembles a changelog from the commits between them, and creates a draft pre-release. For use inside GitHub Actions |
| [`workflow-pack-upload.sh`](#workflow-pack-uploadsh) | CI script: package a compiled binary as `.tar.gz` and upload it to a GitHub release |
| `workflow-pack-upload.ps1` | PowerShell equivalent of `workflow-pack-upload.sh` for Windows CI runners |

---

## Build

### `build-release-with-gdal.sh`

```sh
./scripts/build-release-with-gdal.sh [--install]
```

Requires GDAL development libraries — install first with `install-gdal.sh`.

---

## Check / Quality

## Test

### `test-unix.sh`

```sh
./scripts/test-unix.sh [extra-cargo-args]
```

Runs `rustfmt`, `clippy` (binary + lib, multiple feature combinations), and `cargo test` (bins, lib, doc tests).

---

### `test-coverage.sh`

```sh
./scripts/test-coverage.sh [extra-args]
```

Outputs `lcov.info` at the repo root. Skips e2e tests (`e2e_` prefix).

---

### `test-timing.sh`

```sh
./scripts/test-timing.sh [cargo-test-args]
./scripts/test-timing.sh --package versatiles_pipeline
./scripts/test-timing.sh -- my_specific_test
```

Requires the nightly toolchain (`rustup toolchain install nightly`). Outputs a ranked list of the 30 slowest tests and a per-module summary.

---

### `selftest-versatiles.sh`

```sh
./scripts/selftest-versatiles.sh [path-to-binary]
```

Defaults to `versatiles` on `PATH`. Used inside Docker image builds to verify the binary works in the target environment.

---

## Install

### `install-unix.sh`

```sh
curl -Ls "https://github.com/versatiles-org/versatiles-rs/releases/latest/download/install-unix.sh" | sudo sh
```

---

## Release & Maintenance

### `release-package.sh`

```sh
./scripts/release-package.sh              # interactive menu
./scripts/release-package.sh patch        # patch / minor / major / alpha / beta / rc / dev
```

After running, push with `git push origin main --follow-tags` to trigger the CI release workflow.

---

### `clean-target.sh`

```sh
./scripts/clean-target.sh        # keep anything used in the last 14 days
./scripts/clean-target.sh 30     # ...or a period of your choosing
./scripts/clean-target.sh --all  # remove target/ entirely (cargo clean)
```

Requires [`cargo-sweep`](https://github.com/holmgr/cargo-sweep) for the age-based pass:
`cargo install cargo-sweep`.

---

## Analysis & Profiling

## CI / Workflow

### `workflow-pack-upload.sh`

```sh
./scripts/workflow-pack-upload.sh <folder> <filename-stem> <tag>
```

---

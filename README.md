[![Crates.io](https://img.shields.io/crates/v/versatiles?label=crates.io)](https://crates.io/crates/versatiles)
[![Crates.io](https://img.shields.io/crates/d/versatiles?label=downloads)](https://crates.io/crates/versatiles)
[![Code Coverage](https://codecov.io/gh/versatiles-org/versatiles-rs/branch/main/graph/badge.svg?token=IDHAI13M0K)](https://codecov.io/gh/versatiles-org/versatiles-rs)
[![GitHub Workflow Status (with event)](https://img.shields.io/github/actions/workflow/status/versatiles-org/versatiles-rs/ci.yml)](https://github.com/versatiles-org/versatiles-rs/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

# VersaTiles

VersaTiles is a Rust-based tool for processing and serving tile data efficiently. It supports multiple tile formats and offers functionalities for seamless tile handling.

---

## Table of Contents

- [Installation](#installation)
  - [Linux](#linux)
  - [MacOS](#macos)
  - [NixOS](#nixos)
  - [Docker](#docker)
  - [Building with Cargo](#building-with-cargo)
  - [Building from Source](#building-from-source)
- [Usage](#usage)
  - [Convert Tiles](#convert-tiles)
  - [Serve Tiles](#serve-tiles)
  - [VersaTiles Pipeline Language](#versatiles-pipeline-language)
- [Repository Structure](#repository-structure)
- [Using as a Library](#using-as-a-library)
- [Additional Information](#additional-information)
- [Contributing](#contributing)
- [License](#license)

---

## Installation

### Linux

Install VersaTiles using the provided [installation script](https://github.com/versatiles-org/versatiles-rs/blob/main/scripts/install-unix.sh) (that downloads the correct [precompiled binary](https://github.com/versatiles-org/versatiles-rs/releases/latest/)):

```sh
curl -Ls "https://github.com/versatiles-org/versatiles-rs/raw/main/scripts/install-unix.sh" | sudo sh
```

### MacOS

Install VersaTiles via [Homebrew](https://docs.versatiles.org/guides/install_versatiles#homebrew-for-macos):

```sh
brew tap versatiles-org/versatiles
brew install versatiles
```

### NixOS

VersaTiles is available via `nixpkgs` (starting from version 24.05):

[![Packaging status](https://repology.org/badge/vertical-allrepos/versatiles.svg)](https://repology.org/project/versatiles/versions)

Add this snippet to `configuration.nix`:

```nix
environment.systemPackages = with pkgs; [ versatiles ];
```

Alternatively, use it in a shell environment:

```nix
{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [ versatiles ];
}
```

Find more details on [Nix search](https://search.nixos.org/packages?show=versatiles).

### Docker

Pull the latest [Docker image](https://github.com/versatiles-org/versatiles-docker) for easy deployment:

```sh
docker pull versatiles-org/versatiles
```

### Building with Cargo

Ensure you have [Rust installed](https://doc.rust-lang.org/cargo/getting-started/installation.html), then run:

```sh
cargo install versatiles
```

### Building from Source

Clone the repository and build VersaTiles manually:

```sh
git clone https://github.com/versatiles-org/versatiles-rs.git
cd versatiles-rs
cargo build --bin versatiles --release
cp ./target/release/versatiles /usr/local/bin/
```

---

## Usage

Run `versatiles` to see available commands:

```
Usage: versatiles [OPTIONS] <COMMAND>

Commands:
  convert  Convert between different tile containers
  probe    Show information about a tile container
  serve    Serve tiles via HTTP
  help     Show detailed help
```

### Convert Tiles

Convert between different tile formats, e.g. from `*.tar` to `*.versatiles`:

```sh
versatiles convert satellite_tiles.tar satellite_tiles.versatiles
```

### Serve Tiles

You can run a local HTTP server to serve your tile data:

```sh
versatiles serve satellite_tiles.versatiles
```

By default, this starts a simple HTTP server that serves the tiles from the specified container file.

You can also configure the server using a YAML configuration file:
```sh
versatiles serve -c config.yaml
```

This allows you to define multiple tile sources, set custom CORS headers, enable compression, and fine-tune server behavior.

For a full description of all configuration options, see the [configuration reference](https://github.com/versatiles-org/versatiles-rs/blob/main/versatiles/config.md) or run:
```sh
versatiles help config
```

### VersaTiles Pipeline Language

The VersaTiles Pipeline Language (VPL) allows you to define tile-processing pipelines. Operations include merging multiple tile sources, filtering, and modifying tile content.

Example of combining multiple vector tile sources:

```text
from_merged_vector [
   from_container filename="world.versatiles",
   from_container filename="europe.versatiles" | filter level_min=5,
   from_container filename="germany.versatiles"
]
```

More details can be found in [versatiles_pipeline/README.md](https://github.com/versatiles-org/versatiles-rs/blob/main/versatiles_pipeline/README.md).

---

## GDAL support

`versatiles` supports GDAL since v1.0.0. However, this is still experimental.

### Building with GDAL support

Due to the numerous combinations of operating systems, package managers and GDAL versions, we must streamline this ecosystem. If you require GDAL support, we recommend the following:
1. Build GDAL locally by running `./scripts/install-gdal.sh`. This will build and install GDAL in the subfolder `./.toolchain/gdal`
2. Build `versatiles` with the features `gdal` and `bindgen`. We recommend using the scripts `./scripts/build_debug.sh` and `./scripts/build_release.sh`.

---

## Repository Structure

### Code

- **/versatiles/** - Main library and binary
- **/versatiles_container/** - Handles tile containers (`*.versatiles`, `*.mbtiles`, `*.pmtiles`, etc.)
- **/versatiles_core/** - Core data types and utilities
- **/versatiles_derive/** - Derive macros for the library
- **/versatiles_geometry/** - Handles geometric data (OSM, GeoJSON, vector tiles, etc.)
- **/versatiles_image/** - Manages image data (PNG, JPEG, WEBP)
- **/versatiles_pipeline/** - VersaTiles Pipeline for efficient tile processing

### Helpers

- **/docker/** - Dockerfile for Linux builds
- **/scripts/** - Scripts for checking, building, testing, and releasing
- **/testdata/** - Test files for validation

---

## Using as a Library

VersaTiles can be used as a command-line tool or integrated into Rust projects as a library. Check out [crates.io](https://crates.io/crates/versatiles) and [docs.rs](https://docs.rs/versatiles/latest/versatiles/) for more details.

---

## Additional Information

For advanced usage, guides, and detailed documentation, visit the [official documentation](https://docs.versatiles.org/).

---

## Contributing

VersaTiles is actively developed, and contributions are welcome! If you find bugs, need features, or want to contribute, please check the [GitHub repository](https://github.com/versatiles-org/versatiles-rs) and submit an issue or pull request.

---

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

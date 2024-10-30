
[![Crates.io](https://img.shields.io/crates/v/versatiles?label=crates.io)](https://crates.io/crates/versatiles)
[![Crates.io](https://img.shields.io/crates/d/versatiles?label=downloads)](https://crates.io/crates/versatiles)
[![Code Coverage](https://codecov.io/gh/versatiles-org/versatiles-rs/branch/main/graph/badge.svg?token=IDHAI13M0K)](https://codecov.io/gh/versatiles-org/versatiles-rs)
[![GitHub Workflow Status (with event)](https://img.shields.io/github/actions/workflow/status/versatiles-org/versatiles-rs/ci.yml)](https://github.com/versatiles-org/versatiles-rs/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Matrix Chat](https://img.shields.io/matrix/versatiles:matrix.org?label=matrix)](https://matrix.to/#/#versatiles:matrix.org)

# VersaTiles

VersaTiles is a Rust-based project designed for processing and serving tile data efficiently. It supports multiple tile formats and offers various functionalities for handling tile data.

## Installation

### Linux

Use the [installation script](https://github.com/versatiles-org/versatiles-rs/blob/main/helpers/install-linux.sh) to download the correct [precompiled binary](https://github.com/versatiles-org/versatiles-rs/releases/latest/) and copy it to `/usr/local/bin/`:
```shell
curl -Ls "https://github.com/versatiles-org/versatiles-rs/raw/main/helpers/install-linux.sh" | bash
```

### MacOS

Install VersaTiles using [Homebrew](https://github.com/versatiles-org/versatiles-documentation/blob/main/guides/install_versatiles.md#homebrew-for-macos):
```shell
brew tap versatiles-org/versatiles
brew install versatiles
```

### NixOS

VersaTiles is available via nixpkgs beginning with 24.05. An up to date version is part of nixpkgs-unstable.  
For installation add following snippet into your [configuration.nix](https://nixos.org/manual/nixos/stable/#sec-configuration-file):

```shell
environment.systemPackages = with pkgs; [ versatiles ];
```

You can also use versatiles via [shell environments](https://nixos.wiki/wiki/Development_environment_with_nix-shell):

```shell
{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    versatiles
  ];

}
```

Additional information can be found at [Nix search](https://search.nixos.org/packages?channel=unstable&from=0&size=50&sort=relevance&type=packages&query=versatiles).


### Docker

Pull the prepared [Docker Images](https://github.com/versatiles-org/versatiles-docker) for easy deployment:
```shell
docker pull versatiles-org/versatiles
```

## Building from Source

To build VersaTiles from source, ensure you have [Rust](https://doc.rust-lang.org/cargo/getting-started/installation.html) installed. Then, run:
```shell
cargo install versatiles
```

## Usage

Running the `versatiles` command will list all available commands:
```
Usage: versatiles [OPTIONS] <COMMAND>

Commands:
  convert  Convert between different tile containers
  probe    Show information about a tile container
  serve    Serve tiles via http
  help     Show detailed help
```

## Examples

### Convert Tiles

Convert between different tile formats:
```shell
versatiles convert --tile-format webp satellite_tiles.tar satellite_tiles.versatiles
```

### Serve Tiles

Serve tiles via HTTP:
```shell
versatiles serve satellite_tiles.versatiles
```

## Repository Structure

### Code

- **/versatiles/** - Main library and binary.
- **/versatiles_container/** - Reading and writing tile containers like `*.versatiles`, `*.mbtiles`, `*.pmtiles`, etc.
- **/versatiles_core/** - Core data types, utilities, and macros.
- **/versatiles_derive/** - Handles derive macros.
- **/versatiles_geometry/** - Manages geometry data, including OSM data, GeoJSON, vector tiles, etc.
- **/versatiles_image/** - Handles image data (PNG, JPEG, WEBP) and image processing.
- **/versatiles_pipeline/** - Manages "VersaTiles Pipelines" for fast tile processing.

### Helpers

- **/docker/** - Contains a Dockerfile for building Linux binaries.
- **/helpers/** - Shell scripts for checking, building, and releasing.
- **/testdata/** - Files used during testing.

## Additional Information

For more details, guides, and advanced usage, please refer to the [official documentation](https://github.com/versatiles-org/versatiles-documentation).

## Development and Contribution

VersaTiles is under active development, and the documentation may not always be up to date. We appreciate your understanding and patience. If you encounter any issues or have questions, feel free to open an issue or contribute to our [code](https://github.com/versatiles-org/versatiles-rs) or [documentation](https://github.com/versatiles-org/versatiles-documentation).

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

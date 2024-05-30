# Versatiles

## Install

### Linux

The [installation script](https://github.com/versatiles-org/versatiles-rs/blob/main/helpers/install-linux.sh) will download the correct [precompiled binary](https://github.com/versatiles-org/versatiles-rs/releases/latest/) and copy it to `/usr/local/bin/`:
```bash
curl -Ls "https://github.com/versatiles-org/versatiles-rs/raw/main/helpers/install-linux.sh" | bash
```

### Mac

You can install Versatiles using [Homebrew](https://github.com/versatiles-org/versatiles-documentation/blob/main/guides/install_versatiles.md#homebrew-for-macos):
```bash
brew tap versatiles-org/versatiles
brew install versatiles
```

### Docker

We have prepared [Docker Images](https://github.com/versatiles-org/versatiles-docker) for easy deployment:
```bash
docker pull versatiles-org/versatiles
```

## Build from Source

To build Versatiles from source, you need [Rust](https://doc.rust-lang.org/cargo/getting-started/installation.html) installed. Then, run the following command:
```bash
cargo install versatiles
```

## Run

Running the `versatiles` command will list all available commands:
```
Usage: versatiles <COMMAND>

Commands:
  convert  Convert between different tile containers
  probe    Show information about a tile container
  serve    Serve tiles via http
```

## Examples

### Convert Tiles
Convert between different tile formats:
```bash
versatiles convert --tile-format webp satellite_tiles.tar satellite_tiles.versatiles
```

### Serve Tiles
Serve tiles via HTTP:
```bash
versatiles serve satellite_tiles.versatiles
```

## Additional Information

For more details, guides, and advanced usage, please refer to the [official documentation](https://github.com/versatiles-org/versatiles-documentation).

## Note on Development and Documentation

Please note that this project is under heavy development, and the documentation may not always be up to date. We appreciate your understanding and patience as we work to improve Versatiles. If you encounter any issues or have questions, feel free to open an issue or contribute improvements to our [code](https://github.com/versatiles-org/versatiles-rs) or [documentation](https://github.com/versatiles-org/versatiles-documentation).

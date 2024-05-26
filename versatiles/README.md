
# Install

- You need [Rust](https://doc.rust-lang.org/cargo/getting-started/installation.html)
- run `cargo install versatiles`

## Alternatively:

- You can also use the latest [precompiled binary releases](https://github.com/versatiles-org/versatiles-rs/releases/latest/).
- You can also use [Homebrew (Mac)](https://github.com/versatiles-org/versatiles-documentation/blob/main/guides/install_versatiles.md#homebrew-for-macos)
- And we have prepared [some Docker Images](https://github.com/versatiles-org/versatiles-docker).

Example: Download and install the latest version for Debian on Intel
```bash
curl -sL https://github.com/versatiles-org/versatiles-rs/releases/latest/download/versatiles-linux-gnu-x86_64.tar.gz | tar -xzf - -C /usr/local/bin/
```

# Run

Running `versatiles` will list you all available commands:
```
Usage: versatiles <COMMAND>

Commands:
   convert  Convert between different tile containers
   probe    Show information about a tile container
   serve    Serve tiles via http
```

# examples

```bash
versatiles convert --tile-format webp satellite_tiles.tar satellite_tiles.versatiles

versatiles serve satellite_tiles.versatiles
```

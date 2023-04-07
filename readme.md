
[![Crates.io](https://img.shields.io/crates/v/versatiles?style=flat-square)](https://crates.io/crates/versatiles)
[![Crates.io](https://img.shields.io/crates/d/versatiles?style=flat-square)](https://crates.io/crates/versatiles)
[![License](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)

# install

- Install [Rust](https://doc.rust-lang.org/cargo/getting-started/installation.html)
- Then run `cargo install versatiles`

# run

running `versatiles` will list you the available commands:
```
Usage: versatiles <COMMAND>

Commands:
  convert  Convert between different tile containers
  probe    Show information about a tile container
  serve    Serve tiles via http
```

# formats

| feature             | versatiles | mbtiles | tar |
|---------------------|------------|---------|-----|
| **read container**  |            |         |     |
| - from file         | ✅          | ✅       | ✅   |
| - from http         | ✅          | 🚫      | 🚫  |
| - from gcs          | 🚧         | 🚫      | 🚫  |
| - from S3           | 🚧         | 🚫      | 🚫  |
| **write container** |            |         |     |
| - to file           | ✅          | 🚫      | ✅   |
| **compression**     |            |         |     |
| - uncompressed      | ✅          | 🚫      | ✅   |
| - gzip              | ✅          | ✅       | ✅   |
| - brotli            | ✅          | 🚫      | ✅   |

More about the VersaTiles container format: [github.com/versatiles-org/**versatiles-spec**](https://github.com/versatiles-org/versatiles-spec)

# examples

```bash
versatiles convert --tile-format webp satellite_tiles.tar satellite_tiles.versatiles

versatiles serve satellite_tiles.versatiles
```

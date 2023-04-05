
[![Crates.io](https://img.shields.io/crates/v/versatiles?style=flat-square)](https://crates.io/crates/versatiles)
[![Crates.io](https://img.shields.io/crates/d/versatiles?style=flat-square)](https://crates.io/crates/versatiles)
[![License](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)

# install

- Install [Rust](https://doc.rust-lang.org/cargo/getting-started/installation.html) (very easy)
- Then run `cargo install versatiles` (very easy, but compiling can take 1-2 minutes)

# run

running `versatiles` will list you the available commands:
```
Usage: versatiles <COMMAND>

Commands:
  convert  Convert between different tile containers
  serve    Serve tiles via http
  probe    Show information about a tile container
  compare  Compare two tile containers
```

# formats

| feature             | versatiles | pmtiles | mbtiles | tar |
|---------------------|------------|---------|---------|-----|
| **read container**  |            |         |         |     |
| - from file         | ✅          | 🚧      | ✅       | ✅   |
| - from http         | 🚧         | 🚧      | 🚫      | 🚫  |
| - from gcs          | 🚧         | 🚧      | 🚫      | 🚫  |
| - from S3           | 🚧         | 🚧      | 🚫      | 🚫  |
| **write container** |            |         |         |     |
| - to file           | ✅          | 🚧      | 🚧      | ✅   |
| **precompression**  |            |         |         |     |
| - uncompressed      | ✅          | 🚧      | 🚫      | ✅   |
| - gzip              | ✅          | 🚧      | ✅       | ✅   |
| - brotli            | ✅          | 🚧      | 🚫      | ✅   |

More on the versatiles container: [github.com/versatiles-org/**versatiles-spec**](https://github.com/versatiles-org/versatiles-spec)

# examples

```bash
cargo build && ./target/debug/versatiles convert --tile-format webp tiles/original/hitzekarte.tar tiles/hitzekarte.tar
cargo build && ./target/debug/versatiles convert tiles/original/stuttgart.mbtiles tiles/stuttgart.versatiles
cargo build && ./target/debug/versatiles convert tiles/stuttgart.versatiles tiles/stuttgart.tar
cargo build && ./target/debug/versatiles convert --min-zoom 14 --bbox -30,15,-20,20 ~/Dropbox/Dropbox\ upload/Dropbbox\ upload\ new/versatiles/mbtiles/2023-01-planet.mbtiles tiles/mostly_water.versatiles

cargo build && ./target/debug/versatiles probe tiles/stuttgart.versatiles
cargo build && ./target/debug/versatiles serve tiles/stuttgart.versatiles
cargo build && ./target/debug/versatiles serve -s tiles/frontend tiles/stuttgart.versatiles

cargo build && ./target/debug/versatiles serve -s tiles/frontend tiles/original/europe.mbtiles

cargo instruments --all-features -t "CPU Profiler" -- convert ~/Dropbox/Dropbox\ upload/Dropbbox\ upload\ new/versatiles/mbtiles/2023-01-eu-de.mbtiles tiles/test.versatiles

cargo build -r && ./target/release/versatiles probe --scan ~/Dropbox/Dropbox\ upload/Dropbbox\ upload\ new/versatiles/mbtiles/2023-01-eu-de.mbtiles
cargo instruments --all-features -t "CPU Profiler" -- probe --scan ~/Dropbox/Dropbox\ upload/Dropbbox\ upload\ new/versatiles/mbtiles/2023-01-eu-de.mbtiles

cargo build && ./target/debug/versatiles convert --bbox 2.4,45.5,24.0,55.7 ~/Dropbox/Dropbox\ upload/Dropbbox\ upload\ new/versatiles/mbtiles/2023-01-planet.mbtiles ./tiles/test.versatiles

cargo publish --no-verify
cargo test
cargo bench --bench main

```

# dev config

```
git config --local core.hooksPath .githooks/
```


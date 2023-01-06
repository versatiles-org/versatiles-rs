
# install

- Install [Rust](https://doc.rust-lang.org/cargo/getting-started/installation.html) (very easy)
- Then run `cargo install opencloudtiles` (very easy, but compiling can take 1-2 minutes)

# run

running `opencloudtiles` will list you the available commands:
```
Usage: opencloudtiles <COMMAND>

Commands:
  convert  Convert between different tile containers
  serve    Serve tiles via http
  probe    Show information about a tile container
  compare  Compare two tile containers
```

# formats

| feature             | cloudtiles | pmtiles | mbtiles | tar |
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

More on the cloudtiles container: [github.com/OpenCloudTiles/**opencloudtiles-specification**](https://github.com/OpenCloudTiles/opencloudtiles-specification)

# examples

```bash
cargo build && target/debug/opencloudtiles convert --tile-format webp tiles/original/hitzekarte.tar tiles/hitzekarte.tar
cargo build && target/debug/opencloudtiles convert tiles/original/stuttgart.mbtiles tiles/stuttgart.cloudtiles
cargo build && target/debug/opencloudtiles convert tiles/stuttgart.cloudtiles tiles/stuttgart.tar
cargo build && target/debug/opencloudtiles probe tiles/stuttgart.cloudtiles
cargo build && target/debug/opencloudtiles serve tiles/stuttgart.cloudtiles
cargo build && target/debug/opencloudtiles serve -s tiles/frontend tiles/stuttgart.cloudtiles

cargo build && target/debug/opencloudtiles serve -s tiles/frontend tiles/original/europe.mbtiles

# cargo instruments --all-features -t "CPU Profiler" -- --max-zoom 3 convert tiles/philippines.mbtiles tiles/philippines.cloudtiles

# cargo instruments --all-features -t "CPU Profiler" -- convert tiles/philippines.mbtiles tiles/philippines.cloudtiles
```

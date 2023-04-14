
[![Crates.io](https://img.shields.io/crates/v/versatiles)](https://crates.io/crates/versatiles)
[![Crates.io](https://img.shields.io/crates/d/versatiles)](https://crates.io/crates/versatiles)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Code Coverage](https://codecov.io/gh/versatiles-org/versatiles-rs/branch/main/graph/badge.svg?token=IDHAI13M0K)](https://codecov.io/gh/versatiles-org/versatiles-rs)

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

# supported formats

<table>
   <thead>
      <tr><th>feature</th><th>versatiles</th><th>mbtiles</th><th>tar</th></tr>
   </thead>
   <tbody>
      <tr><th colspan="4" style="text-align:center">read container</th></tr>
      <tr><td>from file</td><td>✅</td><td>✅</td><td>✅</td></tr>
      <tr><td>from http</td><td>✅</td><td>🚫</td><td>🚫</td></tr>
      <tr><td>from gcs</td><td>🚧</td><td>🚫</td><td>🚫</td></tr>
      <tr><td>from S3</td><td>🚧</td><td>🚫</td><td>🚫</td></tr>
      <tr><th colspan="4" style="text-align:center">write container</th></tr>
      <tr><td>to file</td><td>✅</td><td>🚫</td><td>✅</td></tr>
      <tr><th colspan="4" style="text-align:center">compression</th></tr>
      <tr><td>uncompressed</td><td>✅</td><td>🚫</td><td>✅</td></tr>
      <tr><td>gzip</td><td>✅</td><td>✅</td><td>✅</td></tr>
      <tr><td>brotli</td><td>✅</td><td>🚫</td><td>✅</td></tr>
   </tbody>
</table>

More about the VersaTiles container format: [github.com/versatiles-org/**versatiles-spec**](https://github.com/versatiles-org/versatiles-spec)

# examples

```bash
versatiles convert --tile-format webp satellite_tiles.tar satellite_tiles.versatiles

versatiles serve satellite_tiles.versatiles
```

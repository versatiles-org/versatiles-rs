
# Install

- You need [Rust](https://doc.rust-lang.org/cargo/getting-started/installation.html)
- run `cargo install versatiles`

## Alternatively:

- You can also use the latest [precompiled binary releases](https://github.com/versatiles-org/versatiles-rs/releases/latest/).
- You can also use [Homebrew (Mac)](https://github.com/versatiles-org/versatiles-documentation/blob/main/guides/install_versatiles.md#homebrew-for-macos)
- And we have prepared [some Docker Images](https://github.com/versatiles-org/versatiles-docker).

# Run

Running `versatiles` will list you all available commands:
```
Usage: versatiles <COMMAND>

Commands:
   convert  Convert between different tile containers
   probe    Show information about a tile container
   serve    Serve tiles via http
```

# supported file formats

<table>
   <thead>
      <tr><th>Feature</th><th>.versatiles</th><th>.mbtiles</th><th>.tar</th></tr>
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


# build

```bash
cargo build && target/debug/cloudtiles --max-zoom 3 convert tiles/maptiler-osm-2017-07-03-v3.6.1-germany_berlin.mbtiles tiles/berlin.cloudtiles

# cargo instruments --all-features -t "CPU Profiler" -- --max-zoom 3 convert tiles/maptiler-osm-2017-07-03-v3.6.1-germany_berlin.mbtiles tiles/berlin.cloudtiles
```

# format

![file format](docs/file_format.svg)

- integers are stored with little endian byte order
- strings are stored with utf8 encoding

## file header (48 bytes)

| offset | length | type   | description                  |
| ------ | ------ | ------ | ---------------------------- |
| 0      | 24     | string | `"OpenCloudTiles/Container"` |
| 24     | 1      | u8     | version number (= 1)         |
| 25     | 1      | u8     | `tile_format`                |
| 26     | 1      | u8     | `tile_precompression`        |
| 27     | 101    | blob   | empty, fill with zeros       |
| 128    | 8      | u64    | offset of meta_blob          |
| 136    | 8      | u64    | length of meta_blob          |
| 144    | 8      | u64    | offset of root_index         |
| 152    | 8      | u64    | length of root_index         |
| 160    | 96     | blob   | empty, fill with zeros       |

`tile_format` values:
  - `0`: pbf
  - `1`: png
  - `2`: jpg
  - `3`: webp

`tile_precompression` values:
  - `0`: uncompressed
  - `1`: gzip compressed
  - `2`: brotli compressed

## meta_blob

`tiles.json`, compressed with brotli

## root_index

brotli compressed data structure:

| offset  | length | type | description             |
| ------- | ------ | ---- | ----------------------- |
| 0       | 8      | u64  | minimum level           |
| 8       | 8      | u64  | maximum level           |
| 16+i*16 | 8      | u64  | offset of level_index i |
| 24+i*16 | 8      | u64  | length of level_index i |

## level_index

brotli compressed data structure:

| offset  | length | type | description           |
| ------- | ------ | ---- | --------------------- |
| 0       | 8      | u64  | minimum column        |
| 8       | 8      | u64  | maximum column        |
| 16      | 8      | u64  | minimum row           |
| 24      | 8      | u64  | maximum row           |
| 32+i*16 | 8      | u64  | offset of row_index i |
| 40+i*16 | 8      | u64  | length of row_index i |

## row_index

brotli compressed data structure:

| offset | length | type | description           |
| ------ | ------ | ---- | --------------------- |
| 0+i*8  | 8      | u64  | offset of tile_blob i |
| 8+i*8  | 8      | u64  | length of tile_blob i |

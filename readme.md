
# build

```bash
cargo build && target/debug/cloudtiles convert tiles/maptiler-osm-2017-07-03-v3.6.1-germany_berlin.mbtiles tiles/berlin.cloudtiles

# cargo instruments --all-features -t "CPU Profiler" convert tiles/maptiler-osm-2017-07-03-v3.6.1-germany_berlin.mbtiles tiles/berlin.cloudtiles
```

# format

![file format](docs/file_format.svg)

- integers are stored with little endian byte order
- strings are stored with utf8 encoding

## file header (48 bytes)

| offset | length | type   | description          |
| ------ | ------ | ------ | -------------------- |
| 0      | 14     | string | `"OpenCloudTiles"`   |
| 14     | 1      | u8     | version number (= 0) |
| 15     | 1      | u8     | `format`             |
| 16     | 8      | u64    | offset of meta_blob  |
| 16     | 8      | u64    | length of meta_blob  |
| 24     | 8      | u64    | offset of root_index |
| 32     | 8      | u64    | length of root_index |

`format` values:
  - `0`: brotli compressed pbf
  - `1`: png
  - `2`: jpg
  - `3`: webp

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

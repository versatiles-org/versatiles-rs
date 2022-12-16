
# build

```bash
cargo build && target/debug/cloudtiles convert tiles/maptiler-osm-2017-07-03-v3.6.1-germany_berlin.mbtiles tiles/berlin.cloudtiles
```

# format

![file format](docs/file_format.svg)

## file header (16 bytes)

| offset | length | type   | description          |
| ------ | ------ | ------ | -------------------- |
| 0      | 14     | string | "OpenCloudTiles"     |
| 14     | 1      | u8     | version number (= 0) |
| 15     | 1      | u8     | tile type            |

tile type:
- 0: png
- 1: jpg
- 2: brotli compressed pbf
  - 
## file index (16 bytes)

| offset | length | type | description          |
| ------ | ------ | ---- | -------------------- |
| 0      | 8      | u64  | length of meta blob  |
| 8      | 8      | u64  | length of root block |

## meta blob

`tiles.json`, utf8, brotli compressed

## root index

brotli compressed data structure:

| offset  | length | type | description           |
| ------- | ------ | ---- | --------------------- |
| 0       | 8      | u64  | minimum level         |
| 8       | 8      | u64  | maximum level         |
| 8+i*16  | 8      | u64  | size of level block i |
| 16+i*16 | 8      | u64  | size of level index i |

## level index

brotli compressed data structure:

| offset  | length | type | description         |
| ------- | ------ | ---- | ------------------- |
| 0       | 8      | u64  | minimum row         |
| 8       | 8      | u64  | maximum row         |
| 8+i*16  | 8      | u64  | size of row block i |
| 16+i*16 | 8      | u64  | size of row index i |

## row index

brotli compressed data structure:

| offset | length | type | description           |
| ------ | ------ | ---- | --------------------- |
| 0      | 8      | u64  | minimum tile          |
| 8      | 8      | u64  | maximum tile          |
| 8+i*8  | 8      | u64  | offset of tile blob i |
| 16+i*8 | 8      | u64  | size of tile blob i   |

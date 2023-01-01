
# build

```bash
cargo build && target/debug/cloudtiles convert --tile-format webp tiles/original/hitzekarte.tar tiles/hitzekarte.tar
cargo build && target/debug/cloudtiles convert tiles/original/stuttgart.mbtiles tiles/stuttgart.cloudtiles
cargo build && target/debug/cloudtiles convert tiles/stuttgart.cloudtiles tiles/stuttgart.tar
cargo build && target/debug/cloudtiles serve tiles/stuttgart.cloudtiles

# cargo instruments --all-features -t "CPU Profiler" -- --max-zoom 3 convert tiles/philippines.mbtiles tiles/philippines.cloudtiles

# cargo instruments --all-features -t "CPU Profiler" -- convert tiles/philippines.mbtiles tiles/philippines.cloudtiles
```

# format

- all integers are stored in big endian byte order
- strings are stored with utf8 encoding

The file is composed of several parts:
1. A **header** with 256 bytes
2. brotli compressed **metadata** (tiles.json)
3. several **blocks**, where each block consists of:
   - several **tiles**
   - **index** of these tiles
4. **index** of all blocks


<p align="center"><img src="docs/file_format.svg?raw=true"></p>

## file

### `file_header` (62 bytes)

- all `offset`s are relative to start of the file
  
| offset | length | type   | description                      |
|--------|--------|--------|----------------------------------|
| 0      | 28     | string | `"OpenCloudTiles-Container-v1:"` |
| 28     | 1      | u8     | `tile_format`                    |
| 29     | 1      | u8     | `tile_precompression`            |
| 30     | 8      | u64    | `offset` of `meta`               |
| 38     | 8      | u64    | `length` of `meta`               |
| 46     | 8      | u64    | `offset` of `block_index`        |
| 54     | 8      | u64    | `length` of `block_index`        |

### `tile_format` values:
  - `0`: png
  - `1`: jpg
  - `2`: webp
  - `16`: pbf

### `tile_precompression` values:
  - `0`: uncompressed
  - `1`: gzip compressed
  - `2`: brotli compressed

### `meta`

- Content of `tiles.json`
- Compressed with `$tile_precompression`

### `block`

- Each `block` is like a "super tile" and contains data of up to 256x256 (= 65536) `tile`s.

### `block_index` (29 bytes per block)

- Brotli compressed data structure
- Empty `block`s are not stored
- For each block `block_index` contains a 259 bytes long record:

| offset    | length | type | description              |
|-----------|--------|------|--------------------------|
| 0 + 29*i  | 1      | u8   | `level`                  |
| 1 + 29*i  | 4      | u32  | `column`/256             |
| 5 + 29*i  | 4      | u32  | `row`/256                |
| 9 + 29*i  | 1      | u8   | `col_min`                |
| 10 + 29*i | 1      | u8   | `row_min`                |
| 11 + 29*i | 1      | u8   | `col_max`                |
| 12 + 29*i | 1      | u8   | `row_max`                |
| 13 + 29*i | 8      | u64  | `offset` of `tile_index` |
| 21 + 29*i | 8      | u64  | `length` of `tile_index` |

## `block`

- Each `block` contains data of up to 256x256 (= 65536) `tile`s.
- Levels 0-8 can be stored with one `block` each. level 9 might contain 512x512 `tile`s so 4 `block`s are necessary.

<p align="center"><img src="docs/level_blocks.svg?raw=true"></p>

- Each `block` contains the concatenated `tile` blobs and ends with a `tile_index`.
- Neither the order of `block`s in the `file` nor the order of `tile`s in a `block` matters as long as their indexes are correct.
- Note: To efficiently find the `block` that contains the `tile` you are looking for, use a data structure such as a "map", "dictionary", or "associative array" and fill it with the data from the `block_index`.

### `tile`

- each tile is a PNG/PBF/… file as data blob
- precompressed with `$tile_precompression`

### `tile_index`

- brotli compressed data structure
- `tile`s are read horizontally then vertically
- `j = (row - row_min)*(col_max - col_min + 1) + (col - col_min)`

<p align="center"><img src="docs/block_tiles.svg?raw=true"></p>

- identical `tile`s can be stored once and referenced multiple times to save storage space
- if a `tile` does not exist, the length of `tile` is zero

| offset | length | type | description               |
|--------|--------|------|---------------------------|
| 12*j   | 8      | u64  | `offset` of `tile_blob` j |
| 12*j   | 4      | u32  | `length` of `tile_blob` j |

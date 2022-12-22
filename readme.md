
# build

```bash
cargo build && target/debug/cloudtiles convert tiles/philippines.mbtiles tiles/philippines.cloudtiles
cargo build && target/debug/cloudtiles --precompress brotli convert tiles/philippines.mbtiles tiles/philippines.cloudtiles

# cargo instruments --all-features -t "CPU Profiler" -- --max-zoom 3 convert tiles/philippines.mbtiles tiles/philippines.cloudtiles

# cargo instruments --all-features -t "CPU Profiler" -- convert tiles/philippines.mbtiles tiles/philippines.cloudtiles
```

# format

- integers are stored with little endian byte order
- strings are stored with utf8 encoding
- all `offset`s are relative to start of the file

The file is composed of several parts:
1. A **header** with 256 bytes
2. brotli compressed **metadata** (tiles.json)
3. several **blocks**, where each block consists of:
   - several **tiles**
   - **index** of these tiles
4. **index** of all blocks


<p align="center"><img src="docs/file_format.svg?raw=true" class="fix-dark-mode"></p>

## `header` (256 bytes)

| offset | length | type   | description                        |
| ------ | ------ | ------ | ---------------------------------- |
| 0      | 30     | string | `"OpenCloudTiles-Container-v1   "` |
| 30     | 1      | u8     | `tile_format`                      |
| 31     | 1      | u8     | `tile_precompression`              |
| 32     | 96     | blob   | empty space, filled with zeros     |
| 128    | 8      | u64    | `offset` of `meta`                 |
| 136    | 8      | u64    | `length` of `meta`                 |
| 144    | 8      | u64    | `offset` of `block_index`          |
| 152    | 8      | u64    | `length` of `block_index`          |
| 160    | 96     | blob   | empty space, filled with zeros     |

### `tile_format` values:
  - `0`: png
  - `1`: jpg
  - `2`: webp
  - `16`: pbf

### `tile_precompression` values:
  - `0`: uncompressed
  - `1`: gzip compressed
  - `2`: brotli compressed

## `meta`

- content of `tiles.json`
- compressed with `$tile_precompression`

## `block`

- each `block` contains data of up to 256x256 (= 65536) `tile`s
- so it's like a "super tile"
- levels 0-8 can be stored with one `block` each. level 9 might contain 512x512 `tile`s, so 4 `block`s are necessary.

<p align="center"><img src="docs/level_blocks.svg?raw=true" class="fix-dark-mode"></p>

- each `block` contains the concatenated `tile` blobs and ends with a `tile_index`
- empty `block`s are not stored
- Note: To efficiently find the `block` that contains the `tile` you are looking for, use a data structure such as a "map", "dictionary", or "associative array" and fill it with the data from the `block_index`

## `block_index` (40 bytes x n)

- brotli compressed data structure
- offsets are relative to the start of file
- for each block `block_index` contains a 48 bytes long record:

| offset    | length | type | description              |
| --------- | ------ | ---- | ------------------------ |
| 0 + 40*i  | 8      | u64  | `level`                  |
| 8 + 40*i  | 8      | u64  | `row`/256                |
| 16 + 40*i | 8      | u64  | `column`/256             |
| 24 + 40*i | 8      | u64  | `offset` of `tile_index` |
| 32 + 40*i | 8      | u64  | `length` of `tile_index` |

## `tile`

- each tile is a PNG/PBF/… file as data blob
- precompressed with `$tile_precompression`

## `tile_index`

- brotli compressed data structure
- `tile`s are read horizontally then vertically
- `j = (row - min_row)*(max_col - min_col + 1) + (col - min_col)`

<p align="center"><img src="docs/block_tiles.svg?raw=true" class="fix-dark-mode"></p>

- identical `tile`s can be stored once and referenced multiple times to save storage space
- if a `tile` does not exist, the length of `tile` is zero

| offset    | length | type | description               |
| --------- | ------ | ---- | ------------------------- |
| 0         | 1      | u8   | `min_row`                 |
| 1         | 1      | u8   | `max_row`                 |
| 2         | 1      | u8   | `min_column`              |
| 3         | 1      | u8   | `max_column`              |
| 4 + 12*j  | 8      | u64  | `offset` of `tile_blob` j |
| 12 + 12*j | 4      | u32  | `length` of `tile_blob` j |

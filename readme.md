
# build

```bash
cargo build && target/debug/cloudtiles convert tiles/stuttgart.mbtiles tiles/stuttgart.cloudtiles
cargo build && target/debug/cloudtiles convert tiles/hitzekarte.tar tiles/hitzekarte.cloudtiles
cargo build && target/debug/cloudtiles --precompress brotli convert tiles/philippines.mbtiles tiles/philippines.cloudtiles

# cargo instruments --all-features -t "CPU Profiler" -- --max-zoom 3 convert tiles/philippines.mbtiles tiles/philippines.cloudtiles

# cargo instruments --all-features -t "CPU Profiler" -- convert tiles/philippines.mbtiles tiles/philippines.cloudtiles
```

# format

- integers are stored with little endian byte order
- strings are stored with utf8 encoding

The file is composed of several parts:
1. A **header** with 256 bytes
2. brotli compressed **metadata** (tiles.json)
3. several **blocks**, where each block consists of:
   - several **tiles**
   - **index** of these tiles
4. **index** of all blocks


<p align="center"><img src="docs/file_format.svg?raw=true" class="fix-dark-mode"></p>

## file

### `file_header` (62 bytes)

- all `offset`s are relative to start of the file
  
| offset | length | type   | description                             |
| ------ | ------ | ------ | --------------------------------------- |
| 0      | 28     | string | `"OpenCloudTiles-Container-v1:"`        |
| 28     | 1      | u8     | `tile_format`                           |
| 29     | 1      | u8     | `tile_precompression`                   |
| 30     | 8      | u64    | `offset` of `meta` (in the file)        |
| 38     | 8      | u64    | `length` of `meta`                      |
| 46     | 8      | u64    | `offset` of `block_index` (in the file) |
| 54     | 8      | u64    | `length` of `block_index`               |

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

### `block_index` (25 bytes per block)

- Brotli compressed data structure
- Offsets are relative to the start of file
- Empty `block`s are not stored
- For each block `block_index` contains a 25 bytes long record:

| offset    | length | type | description                            |
| --------- | ------ | ---- | -------------------------------------- |
| 0 + 25*i  | 1      | u8   | `level`                                |
| 1 + 25*i  | 2      | u16  | `row`/256                              |
| 3 + 25*i  | 2      | u16  | `column`/256                           |
| 5 + 25*i  | 8      | u64  | `offset` of `tile_index`, in the block |
| 13 + 25*i | 4      | u32  | `length` of `tile_index`               |
| 17 + 25*i | 8      | u64  | `offset` of `block`, in the file       |

## `block`

- Each `block` contains data of up to 256x256 (= 65536) `tile`s.
- Levels 0-8 can be stored with one `block` each. level 9 might contain 512x512 `tile`s so 4 `block`s are necessary.

<p align="center"><img src="docs/level_blocks.svg?raw=true" class="fix-dark-mode"></p>

- Each `block` contains the concatenated `tile` blobs and ends with a `tile_index`.
- Neither the order of `block`s in the `file` nor the order of `tile`s in a `block` matters as long as their indexes are correct.
- Note: To efficiently find the `block` that contains the `tile` you are looking for, use a data structure such as a "map", "dictionary", or "associative array" and fill it with the data from the `block_index`.

### `tile`

- each tile is a PNG/PBF/… file as data blob
- precompressed with `$tile_precompression`

### `tile_index`

- brotli compressed data structure
- `tile`s are read horizontally then vertically
- `j = (row - min_row)*(max_col - min_col + 1) + (col - min_col)`

<p align="center"><img src="docs/block_tiles.svg?raw=true" class="fix-dark-mode"></p>

- identical `tile`s can be stored once and referenced multiple times to save storage space
- if a `tile` does not exist, the length of `tile` is zero

| offset  | length | type | description                           |
| ------- | ------ | ---- | ------------------------------------- |
| 0       | 1      | u8   | `min_row`                             |
| 1       | 1      | u8   | `max_row`                             |
| 2       | 1      | u8   | `min_column`                          |
| 3       | 1      | u8   | `max_column`                          |
| 4 + 8*j | 5      | u40  | `offset` of `tile_blob` j, in `block` |
| 9 + 8*j | 3      | u24  | `length` of `tile_blob` j             |

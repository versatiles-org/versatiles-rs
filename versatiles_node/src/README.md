# Source Directory

The Rust source of the Node.js bindings, and the TypeScript tests for them. How
to build, test and contribute is in [CONTRIBUTING.md](../CONTRIBUTING.md).

## Rust

Each `#[napi]` item here becomes part of the JavaScript API. NAPI-RS generates
`index.d.ts` from them, so their doc comments are what users see in their
editor.

| File             | What it holds                                                                     |
| ---------------- | --------------------------------------------------------------------------------- |
| `lib.rs`         | Module entry point                                                                |
| `tile_source.rs` | `TileSource`: open a container or VPL pipeline, read tiles, metadata, `convertTo` |
| `convert.rs`     | The standalone `convert()` function                                               |
| `server.rs`      | `TileServer`: the HTTP tile server                                                |
| `progress.rs`    | `ProgressData` and `MessageData`, passed to the conversion callbacks              |
| `layer_stats.rs` | `layerStats()`: per-layer byte breakdown of a vector tile                         |
| `vpl.rs`         | `parseVpl()` and friends: VPL text to structure and back                          |
| `codegen.rs`     | Generates the typed VPL builder (`src/vpl.ts`) from the Rust operation metadata   |
| `runtime.rs`     | The runtime every source and conversion runs on                                   |
| `macros.rs`      | Converting Rust errors into JavaScript errors                                     |
| `types/`         | Option objects, `TileCoord`, `TileJSON`, metadata and their validation            |

## TypeScript

| File        | What it holds                                                             |
| ----------- | ------------------------------------------------------------------------- |
| `*.test.ts` | Tests, run with [vitest](https://vitest.dev) against the built `index.js` |
| `vpl.ts`    | **Generated** by `npm run build:vpl` — edit `codegen.rs`, not this file   |

Run every test with `npm test`, or one file with `npx vitest run src/server.test.ts`.
The tests load the native module, so build it first with `npm run build:debug`.

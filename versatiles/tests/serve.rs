//! End-to-end tests that run the built binary as a server.
//!
//! One test target rather than six: each integration test file becomes its own
//! binary, and every one of them links the whole crate — axum, reqwest, tokio,
//! and GDAL when it is enabled. The submodules below were those binaries.
//! Tests still run concurrently inside this one, and the server takes `-p 0`,
//! so nothing here depends on a fixed port.

#![cfg(all(feature = "cli", feature = "server"))]

// `#[macro_use]` so the submodules below see `assert_contains!`, which each
// file used to get for free by being its own crate root.
#[macro_use]
mod test_utilities;

// `#[path]` because a `mod` in a test target resolves against `tests/`, where a
// bare file would become a test binary of its own again.
#[path = "serve/basic.rs"]
mod basic;
#[path = "serve/compression.rs"]
mod compression;
#[path = "serve/conditional.rs"]
mod conditional;
#[path = "serve/config.rs"]
mod config;
#[path = "serve/cors.rs"]
mod cors;
#[path = "serve/empty_tiles.rs"]
mod empty_tiles;
#[path = "serve/shutdown.rs"]
mod shutdown;
#[path = "serve/static_files.rs"]
mod static_files;

// ! This module provides JSON utilities for parsing, reading, and stringifying JSON data, including support for NDJSON (newline-delimited JSON).
// ! It re-exports helper functions and types like `JsonValue`, `JsonArray`, and `JsonObject`.

mod parse;
mod read;
mod stringify;
mod types;

pub use parse::{parse_json_iter, parse_json_str};
pub use read::{read_ndjson_iter, read_ndjson_stream};
pub use stringify::*;
pub use types::{JsonArray, JsonObject, JsonValue};

//! JSON parsing, serialization, and NDJSON utilities
//!
//! This module provides comprehensive JSON support including:
//! - **Parsing**: Convert JSON strings and byte streams to [`JsonValue`]
//! - **Serialization**: Convert [`JsonValue`] to JSON strings
//! - **NDJSON**: Read newline-delimited JSON streams
//! - **Types**: Work with [`JsonValue`], [`JsonArray`], and [`JsonObject`]
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::json::parse_json_str;
//!
//! // Parse JSON from a string
//! let json = parse_json_str(r#"{"name": "example", "count": 42}"#).unwrap();
//!
//! // Access the parsed object
//! assert!(json.as_object().is_ok());
//! ```

mod parse;
mod read;
mod stringify;
mod types;

pub use parse::{parse_json_iter, parse_json_str};
pub use read::{read_ndjson_iter, read_ndjson_stream};
pub use stringify::*;
pub use types::{JsonArray, JsonObject, JsonValue};

//! VersaTiles server configuration system.
//!
//! This module provides the configuration types and parsers for the VersaTiles HTTP server.
//! It includes support for:
//! - [`Config`]: top-level configuration loader and YAML parser
//! - [`ServerConfig`]: network and API settings
//! - [`CorsConfig`]: CORS policy configuration
//! - [`StaticSourceConfig`]: static file sources
//! - [`TileSourceConfig`]: tile data sources
//!
//! These submodules are typically deserialized from a YAML file (`server.yml`)
//! and consumed by the HTTP server during startup.

mod cors;
mod main;
mod pair;
mod path_pattern;
mod server;
mod static_source;
mod tile_source;

pub use cors::{CorsConfig, DEFAULT_CORS_MAX_AGE_SECONDS};
pub use main::Config;
pub use path_pattern::PathPattern;
pub use server::ServerConfig;
pub use static_source::{DEFAULT_STATIC_CACHE_CONTROL, IMMUTABLE_CACHE_CONTROL, StaticSourceConfig};
pub use tile_source::TileSourceConfig;

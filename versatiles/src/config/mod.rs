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
mod server;
mod static_source;
mod tile_source;

pub use cors::CorsConfig;
pub use main::Config;
pub use server::ServerConfig;
pub use static_source::StaticSourceConfig;
pub use tile_source::TileSourceConfig;

# versatiles_derive

Procedural macros for the VersaTiles ecosystem.

[![Crates.io](https://img.shields.io/crates/v/versatiles_derive)](https://crates.io/crates/versatiles_derive)
[![Documentation](https://docs.rs/versatiles_derive/badge.svg)](https://docs.rs/versatiles_derive)

## Overview

This crate provides derive macros and attribute macros used internally by VersaTiles for code generation and ergonomic error handling.

### Provided Macros

- **`#[derive(VPLDecode)]`**: Automatically generates decoding logic for VPL (VersaTiles Pipeline Language) data structures
- **`#[derive(ConfigDoc)]`**: Generates YAML configuration documentation from struct definitions
- **`#[context("...")]`**: Adds contextual error messages to functions returning `Result`

## Usage

This crate is primarily for internal use within the VersaTiles project. If you're using VersaTiles as a library, you typically won't need to use this crate directly.

Add this to your `Cargo.toml` if needed:

```toml
[dependencies]
versatiles_derive = "2.3"
```

### Example

```rust
use versatiles_derive::{VPLDecode, context};
use anyhow::Result;

#[derive(VPLDecode)]
struct MyConfig {
    name: String,
    value: u32,
}

#[context("failed to process data")]
fn process() -> Result<()> {
    // Errors will include "failed to process data" context
    Ok(())
}
```

## API Documentation

For detailed API documentation and macro usage, see [docs.rs/versatiles_derive](https://docs.rs/versatiles_derive).

## Part of VersaTiles

This crate is part of the [VersaTiles](https://github.com/versatiles-org/versatiles-rs) project.

## License

MIT License - see [LICENSE](https://github.com/versatiles-org/versatiles-rs/blob/main/LICENSE) for details.

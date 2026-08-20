//! End-to-end tests that run the built binary as a command.
//!
//! One test target rather than four; see the note in `serve.rs`.

#![cfg(feature = "cli")]
#![allow(clippy::float_cmp)]

// `#[macro_use]` so the submodules below see `assert_contains!`, which each
// file used to get for free by being its own crate root.
#[macro_use]
mod test_utilities;

#[path = "cli/command.rs"]
mod command;
#[path = "cli/convert.rs"]
mod convert;
#[path = "cli/convert_integrity.rs"]
mod convert_integrity;
#[path = "cli/help.rs"]
mod help;

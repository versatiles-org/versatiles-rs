//! Use a directory as a tile container
//!
//! This module provides structures and implementations for reading and writing tiles to and from a directory structure.
//!
//! The main components of this module are:
//! - `DirectoryReader`: Reads tiles from a directory structure.
//! - `DirectoryWriter`: Writes tiles to a directory structure.

mod reader;
mod writer;

pub use reader::DirectoryReader;
pub use writer::DirectoryWriter;

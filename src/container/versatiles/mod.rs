//! `*.versatiles` container
//!
//! see [specification](https://github.com/versatiles-org/versatiles-spec)
//!

mod types;

mod abstract_readers;
pub use abstract_readers::*;

mod abstract_writers;
pub use abstract_writers::*;

mod reader;
pub use reader::VersaTilesReader;

mod writer;
pub use writer::VersaTilesWriter;

//! `*.versatiles` container
//!
//! see [specification](https://github.com/versatiles-org/versatiles-spec)
//!

mod types;

mod reader;
pub use reader::VersaTilesReader;

mod writer;
pub use writer::VersaTilesWriter;

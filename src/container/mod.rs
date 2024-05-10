//! contains various tile container implementations
//!
//! ## Supported tile container formats
//!
//! | Format         | Read | Write | Feature   |
//! |----------------|:----:|:-----:|-----------|
//! | `*.versatiles` | ✅   | ✅     | `default` |
//! | `*.mbtiles`    | ✅   | ⛔️     | `full`    |
//! | `*.pmtiles`    | ⛔️   | ⛔️     | `full`    |
//! | `*.tar`        | ✅   | ✅     | `full`    |
//! | directory      | ✅   | ✅     | `default` |
//!

mod types;
pub use types::*;

mod reader;
pub use reader::*;

mod writer;
pub use writer::*;

mod directory;
pub use directory::*;

mod versatiles;
pub use versatiles::*;

#[cfg(feature = "full")]
#[path = ""]
mod optional_modules {
	mod converter;
	pub use converter::*;

	mod getters;
	#[cfg(test)]
	pub use getters::tests::*;
	pub use getters::*;

	mod mbtiles;
	pub use mbtiles::*;

	mod pmtiles;
	pub use pmtiles::*;

	mod tar;
	pub use tar::*;
}

#[cfg(feature = "full")]
pub use optional_modules::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
pub use mock::*;

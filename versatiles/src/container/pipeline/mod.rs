/*!
The `pipeline` module provides functionality for reading, processing, and composing tiles from multiple sources.
*/

mod operations;
mod reader;
mod utils;

pub use reader::PipelineReader;
use utils::*;

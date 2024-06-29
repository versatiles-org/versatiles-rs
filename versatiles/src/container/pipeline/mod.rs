/*!
The `pipeline` module provides functionality for reading, processing, and composing tiles from multiple sources.
*/

mod operations;
mod reader;
pub mod utils;

pub use reader::PipelineReader;

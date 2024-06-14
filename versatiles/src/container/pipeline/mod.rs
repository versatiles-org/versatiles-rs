/*!
The `pipeline` module provides functionality for reading, processing, and composing tiles from multiple sources.
*/

mod operations;
mod reader;
mod utils;

pub use reader::PipelineReader;
pub use utils::get_pipeline_operation_docs;
use utils::*;

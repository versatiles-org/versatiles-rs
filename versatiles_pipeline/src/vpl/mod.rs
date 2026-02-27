#[cfg(feature = "codegen")]
mod field_meta;
mod parser;
mod vpl_node;
mod vpl_pipeline;

#[cfg(feature = "codegen")]
pub use field_meta::VPLFieldMeta;
pub use parser::parse_vpl;
pub use vpl_node::VPLNode;
pub use vpl_pipeline::VPLPipeline;

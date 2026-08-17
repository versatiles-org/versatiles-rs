//! The **VersaTiles Pipeline Language** (VPL): its syntax tree, parser and serialiser.
//!
//! A pipeline is a sequence of [`VPLNode`]s joined by `|`, each with a name, a multi-valued
//! parameter map, and zero or more child pipelines in square brackets:
//!
//! ```vpl
//! from_stacked [ from_container filename="a.versatiles", from_container filename="b.versatiles" ]
//!   | vector_filter_layers layer=["place", "water"]
//! ```
//!
//! # Reading and writing
//!
//! [`parse_vpl`] turns text into a [`VPLPipeline`], and `Display` turns one back:
//!
//! ```
//! use versatiles_pipeline::vpl::parse_vpl;
//!
//! let pipeline = parse_vpl("from_container filename='world.versatiles' | filter level_min=5")?;
//! assert_eq!(
//!     pipeline.to_string(),
//!     "from_container filename=world.versatiles | filter level_min=5"
//! );
//! # Ok::<(), anyhow::Error>(())
//! ```
//!
//! The two are inverse in meaning but not in text: parsing discards comments, parameter order and
//! formatting, so `to_string` normalises rather than reproduces. See [`VPLPipeline`]'s `Display`
//! for the full list — it matters if you are round-tripping a file someone wrote by hand.
//!
//! # Errors
//!
//! [`parse_vpl`] reports failures as a caret-annotated trace, which is what the CLI prints.
//! [`parse_vpl_detailed`] returns the same failure as [`VplParseError`], which states the position
//! as byte offsets instead of drawing it — what an editor or language server needs.

mod cst;
mod error;
#[cfg(feature = "codegen")]
mod field_meta;
mod parser;
mod serializer;
mod vpl_node;
mod vpl_pipeline;

pub use cst::{
	CstArray, CstFile, CstNode, CstPipeline, CstProperty, CstSources, CstString, CstStringKind, CstToken, Punctuated,
	PunctuatedItem,
};
pub use error::{VplErrorFrame, VplParseError};
#[cfg(feature = "codegen")]
pub use field_meta::VPLFieldMeta;
pub use parser::{parse_vpl, parse_vpl_detailed};
pub use vpl_node::VPLNode;
pub use vpl_pipeline::VPLPipeline;

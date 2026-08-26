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
//! for the full list, and the next section for the tree that keeps all of it.
//!
//! # Two trees
//!
//! There are two representations, and which one you want depends on whether you mean to *run* a
//! pipeline or *edit* one.
//!
//! [`VPLPipeline`] is the **semantic** tree: what an operation needs in order to execute. It
//! deliberately forgets everything the engine has no use for — comments, whitespace, the order the
//! parameters were written in, and which quotes the author chose. [`parse_vpl`] produces it, and
//! its `Display` prints one back.
//!
//! [`CstFile`] is the **concrete** syntax tree: every byte of the source, including the parts the
//! engine ignores. [`parse_cst`] produces it, and printing an unedited tree reproduces the input
//! exactly. Reach for it when the text belongs to a person — a `.vpl` file someone is editing —
//! because saving a semantic tree would rewrite parts nobody touched:
//!
//! ```
//! use versatiles_pipeline::vpl::parse_cst;
//!
//! let source = "# where the data lives\nfrom_container  filename = 'berlin.versatiles'\n";
//! let mut file = parse_cst(source)?;
//!
//! // nothing is lost on the way in
//! assert_eq!(file.to_string(), source);
//!
//! // change one value...
//! file.pipeline.nodes.items[0]
//!     .value
//!     .set_property("filename", "hamburg.versatiles");
//!
//! // ...and only that value changes: the comment and the spacing are still there
//! assert_eq!(
//!     file.to_string(),
//!     "# where the data lives\nfrom_container  filename = hamburg.versatiles\n"
//! );
//!
//! // and it still lowers to something the engine can run
//! assert_eq!(file.lower().len(), 1);
//! # Ok::<(), versatiles_pipeline::vpl::VplParseError>(())
//! ```
//!
//! [`CstFile::format`] lays that same tree out the way
//! [`VPLPipeline::to_string_pretty`] would, keeping the comments — which is what an editor's
//! Format command needs, since the semantic tree has already forgotten them.
//!
//! There is only one grammar: [`parse_vpl`] is [`parse_cst`] followed by
//! [`CstFile::lower`], so the two cannot disagree about what VPL means.
//!
//! # Errors
//!
//! [`parse_vpl`] reports failures as a caret-annotated trace, which is what the CLI prints.
//! [`parse_vpl_detailed`] and [`parse_cst`] return the same failure as [`VplParseError`], which
//! states the position as byte offsets instead of drawing it — what an editor or language server
//! needs.

mod cst;
mod cst_format;
mod cst_parser;
mod error;
mod field_meta;
mod parser;
mod serializer;
mod vpl_node;
mod vpl_pipeline;

pub use cst::{
	CstArray, CstFile, CstNode, CstPipeline, CstProperty, CstSources, CstString, CstStringKind, CstToken, CstValue,
	Punctuated, PunctuatedItem,
};
pub use cst_parser::parse_cst;
pub use error::{VplErrorFrame, VplParseError};
pub use field_meta::{VPLFieldMeta, ValueValidator};
pub use parser::{parse_vpl, parse_vpl_detailed};
pub use vpl_node::{VPLNode, parse_bool};
pub use vpl_pipeline::VPLPipeline;

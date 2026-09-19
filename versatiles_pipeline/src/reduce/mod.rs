//! Reducing a tileset to what one style draws.
//!
//! `@versatiles/style` can say which source-layers, properties and features a built style
//! actually reads (`versatiles-org/versatiles-style#134`). This module reads that requirement
//! and renders the VPL pipeline that strips a tileset down to it.
//!
//! The rendering lives here rather than in the exporter because VPL's vocabulary — operation
//! names, parameter spellings, what `vector_filter_properties`'s regex matches against — is this
//! crate's, and a copy of it elsewhere is a second implementation that agrees until it does not.
//!
//! ## The contract
//!
//! **The rendered pipeline may keep features the style never draws; it must never drop one it
//! does.** Every decision made under uncertainty widens toward keeping. Reading the requirement
//! ([`ir`]) already works that way — an unrecognised operator parses as
//! [`Predicate::Unknown`](ir::Predicate::Unknown) rather than failing — and the renderer turns
//! that into "keep".

mod cel;
pub mod ir;
mod predicate;
mod render;

pub use ir::{KeepEntry, LayerRequirement, Literal, Predicate, Requirement, SourceInfo};
pub use render::operations;

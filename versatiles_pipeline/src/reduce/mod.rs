//! Reducing a tileset to what one style draws.
//!
//! Two halves. [`style`] reads a built MapLibre style and works out which source-layers, properties
//! and features it actually draws; [`render`](render::operations) turns that answer into the VPL
//! operations that strip a tileset down to it.
//!
//! Both live here rather than beside the styles because VPL's vocabulary — operation names,
//! parameter spellings, what `vector_filter_properties`'s regex matches against — is this crate's,
//! and a copy of it elsewhere is a second implementation that agrees until it does not. Keeping the
//! analysis here too means the two never disagree about what a predicate means, and that the
//! tileset's own pyramid is available to clamp zooms against.
//!
//! ## The contract
//!
//! **The reduction may keep features the style never draws; it must never drop one it does.** Every
//! decision made under uncertainty widens toward keeping: a filter that cannot be read becomes
//! "keep everything in this zoom range", and a property that might be read is kept. The one thing
//! that fails rather than widens is a style that draws no tile data at all, where widening would
//! mean producing an empty tileset from what is almost certainly the wrong file.

mod cel;
pub mod ir;
mod predicate;
mod render;
mod style;

pub use ir::{KeepEntry, LayerRequirement, Literal, Predicate, Requirement};
pub use render::operations;
pub use style::from_style;

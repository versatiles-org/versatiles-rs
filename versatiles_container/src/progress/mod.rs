//! Progress reporting.
//!
//! A [`ProgressFactory`] hands out [`ProgressHandle`]s, each of which owns one
//! progress bar and publishes [`ProgressState`] snapshots on the runtime's
//! event bus. Rendering to a terminal is one listener among others, so a
//! library consumer can observe progress without a terminal being involved.

mod factory;
mod handle;
mod types;

pub use factory::ProgressFactory;
pub use handle::ProgressHandle;
pub use types::{ProgressId, ProgressState};

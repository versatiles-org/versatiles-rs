#[cfg(feature = "full")]
mod progress_bar;

mod progress_drain;

mod traits;
pub use traits::{get_progress_bar, ProgressTrait};

//! This module provides the main interface for progress indicators, including conditional compilation
//! for different progress implementations.
//!
//! # Overview
//!
//! The module conditionally includes different progress indicator implementations based on the
//! build configuration. By default, it provides a no-op progress drain. If the "full" feature is
//! enabled, it includes a terminal-based progress bar. The `ProgressTrait` trait defines the
//! common interface for all progress indicators, and the `get_progress_bar` function provides
//! a convenient way to create an instance of a progress indicator.
//!
//! # Examples
//!
//! ```rust
//! use versatiles::utils::progress::{get_progress_bar, ProgressTrait};
//!
//! let mut progress = get_progress_bar("Processing", 100);
//! progress.set_position(50);
//! progress.inc(10);
//! progress.finish();
//! ```

#![allow(unused)]

#[cfg(not(feature = "test"))]
mod progress_bar;
#[cfg(not(feature = "test"))]
pub use progress_bar::ProgressBar;

#[cfg(feature = "test")]
mod progress_drain;
#[cfg(feature = "test")]
pub use progress_drain::ProgressDrain;

mod traits;
pub use traits::{get_progress_bar, ProgressTrait};

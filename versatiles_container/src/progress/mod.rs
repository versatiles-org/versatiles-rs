//! Progress reporting.
//!
//! A [`ProgressFactory`] hands out [`ProgressHandle`]s, each of which owns one
//! progress bar and publishes [`ProgressState`] snapshots on the runtime's
//! event bus. Rendering to a terminal is one listener among others, so a
//! library consumer can observe progress without a terminal being involved.
//!
//! Drawing a bar touches nothing outside the handle. An application that wants
//! the terminal's progress indicator cleared when it panics or is interrupted
//! opts in with [`install_terminal_reset_hooks`], which is process-wide and
//! therefore `main`'s decision to make. One that later wants the signals for
//! itself gives them back with [`release_terminal_reset_signals`].

mod factory;
mod handle;
mod types;

pub use factory::ProgressFactory;
pub use handle::{
	ProgressHandle, install_terminal_reset_hooks, release_terminal_reset_signals, terminal_reset_hooks_installed,
};
pub use types::{ProgressId, ProgressState};

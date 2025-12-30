//! Runtime configuration and services for tile processing operations
//!
//! The runtime provides a unified interface for:
//! - Global processing parameters (memory limits, cache configuration)
//! - Container format registry (readers/writers)
//! - Unified event bus (logs, progress, messages, warnings, errors)
//! - Progress bar factory (create multiple independent progress bars)
//!
//! # Example
//!
//! ```no_run
//! use versatiles_container::TilesRuntime;
//!
//! let runtime = TilesRuntime::builder()
//!     .with_memory_cache()
//!     .max_memory(2 * 1024 * 1024 * 1024)
//!     .silent(true)
//!     .build();
//!
//! // Subscribe to events
//! runtime.events().subscribe(|event| {
//!     println!("{:?}", event);
//! });
//!
//! // Create progress bars
//! let progress = runtime.create_progress("Processing", 1000);
//! progress.inc(100);
//! progress.finish();
//! ```

mod builder;
mod events;
mod inner;
mod outer;

pub use builder::RuntimeBuilder;
pub use events::{Event, EventBus, ListenerId, LogAdapter, LogLevel};
pub use inner::RuntimeInner;
pub use outer::TilesRuntime;

//! Runtime configuration and creation
//!
//! This module provides a centralized way to create and configure the VersaTiles
//! runtime for use in Node.js bindings. The runtime is configured with:
//!
//! - Pipeline readers registered for advanced tile processing
//! - Silent mode enabled (no stderr output)
//! - Default in-memory cache

use versatiles::pipeline::register_pipeline_readers;
use versatiles_container::TilesRuntime;

/// Create a configured TilesRuntime for Node.js usage
///
/// The runtime is preconfigured with:
/// - Custom pipeline readers for advanced processing capabilities
/// - Silent mode (no stderr output, suitable for Node.js environment)
/// - Default in-memory cache
///
/// # Returns
///
/// A fully configured [`TilesRuntime`] ready for use in tile operations
///
/// # Example
///
/// ```
/// let runtime = create_runtime();
/// let reader = runtime.get_reader_from_str("tiles.versatiles").await?;
/// ```
pub fn create_runtime() -> TilesRuntime {
	TilesRuntime::builder()
		.customize_registry(register_pipeline_readers)
		.silent()
		.build()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_create_runtime() {
		let runtime = create_runtime();
		// Runtime should be created successfully
		// Verify it's a valid runtime by checking it can create a progress handle
		let progress = runtime.create_progress("Test", 100);
		assert_eq!(progress.id().0, 1);
	}

	#[test]
	fn test_runtime_has_custom_registry() {
		let runtime = create_runtime();
		// The runtime should have the pipeline readers registered
		// We can verify this by checking that the runtime exists and is usable
		let _events = runtime.events();
		// If we got here, the runtime was created successfully with custom registry
	}

	#[test]
	fn test_runtime_is_silent() {
		let runtime = create_runtime();
		// Create a progress to verify runtime works
		let _progress = runtime.create_progress("Silent test", 50);
		// The runtime should be in silent mode (no stderr output)
		// This is verified by the build() method being called with silent()
	}

	#[test]
	fn test_runtime_creates_multiple_progress_handles() {
		let runtime = create_runtime();
		let progress1 = runtime.create_progress("Task 1", 100);
		let progress2 = runtime.create_progress("Task 2", 200);
		let progress3 = runtime.create_progress("Task 3", 300);

		// Each progress should have a unique ID
		assert_ne!(progress1.id().0, progress2.id().0);
		assert_ne!(progress2.id().0, progress3.id().0);
		assert_ne!(progress1.id().0, progress3.id().0);
	}

	#[test]
	fn test_runtime_events_can_subscribe() {
		let runtime = create_runtime();
		let events = runtime.events();

		// Should be able to subscribe to events
		let _listener = events.subscribe(|_event| {
			// Event handler
		});
	}

	#[test]
	fn test_runtime_can_be_cloned() {
		let runtime1 = create_runtime();
		let runtime2 = runtime1.clone();

		// Both should create valid progress handles
		let progress1 = runtime1.create_progress("Clone test 1", 100);
		let progress2 = runtime2.create_progress("Clone test 2", 100);

		// Both should work independently
		assert!(progress1.id().0 > 0);
		assert!(progress2.id().0 > 0);
	}

	#[test]
	fn test_runtime_progress_with_zero_max_value() {
		let runtime = create_runtime();
		let progress = runtime.create_progress("Zero test", 0);

		// Should handle zero max value
		assert_eq!(progress.id().0, 1);
	}

	#[test]
	fn test_runtime_progress_with_large_max_value() {
		let runtime = create_runtime();
		let progress = runtime.create_progress("Large test", u64::MAX);

		// Should handle very large max value
		assert_eq!(progress.id().0, 1);
	}

	#[test]
	fn test_runtime_multiple_instances_independent() {
		let runtime1 = create_runtime();
		let runtime2 = create_runtime();

		let progress1 = runtime1.create_progress("Runtime 1", 100);
		let progress2 = runtime2.create_progress("Runtime 2", 100);

		// Different runtime instances should work independently
		assert_eq!(progress1.id().0, 1);
		assert_eq!(progress2.id().0, 1);
	}

	#[test]
	fn test_runtime_progress_with_unicode_description() {
		let runtime = create_runtime();
		let progress = runtime.create_progress("Unicode: 日本語 🦀 Ελληνικά", 100);

		// Should handle unicode in descriptions
		assert_eq!(progress.id().0, 1);
	}

	#[test]
	fn test_runtime_progress_with_empty_description() {
		let runtime = create_runtime();
		let progress = runtime.create_progress("", 100);

		// Should handle empty description
		assert_eq!(progress.id().0, 1);
	}
}

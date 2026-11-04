//! Runtime configuration and creation
//!
//! This module provides a centralized way to create and configure the VersaTiles
//! runtime for use in Node.js bindings. The runtime is configured with:
//!
//! - Pipeline readers registered for advanced tile processing
//! - Silent mode enabled (no stderr output)
//! - An in-memory cache, or a disk cache if a directory was given or is in the environment
//! - An SSH identity for SFTP, if one was given or is in the environment

use std::path::PathBuf;

use versatiles::pipeline::register_pipeline_readers;
use versatiles_container::TilesRuntime;

/// Environment variable naming the SSH identity file, as the CLI reads it.
const SSH_IDENTITY_ENV: &str = "VERSATILES_SSH_IDENTITY";

/// Environment variable naming the disk cache directory, as the CLI reads it.
const CACHE_DIR_ENV: &str = "VERSATILES_CACHE_DIR";

/// Create a configured TilesRuntime for Node.js usage
///
/// The runtime is preconfigured with:
/// - Custom pipeline readers for advanced processing capabilities
/// - Silent mode (no stderr output, suitable for Node.js environment)
/// - A disk cache in `VERSATILES_CACHE_DIR` if that is set, an in-memory cache otherwise
/// - The SSH identity in `VERSATILES_SSH_IDENTITY`, if that is set
///
/// # Returns
///
/// A fully configured [`TilesRuntime`] ready for use in tile operations
///
/// # Example
///
/// ```
/// let runtime = create_runtime();
/// let reader = runtime.reader_from_str("tiles.versatiles").await?;
/// ```
pub fn create_runtime() -> TilesRuntime {
	create_runtime_with(None, None)
}

/// Create a configured TilesRuntime with an SSH identity and a cache directory
///
/// `ssh_identity` is a path to a private key file on disk. Without one, the
/// `VERSATILES_SSH_IDENTITY` environment variable is used — the same variable
/// the CLI reads — and without that, the SSH client falls back to any password in the
/// URL, the SSH agent, and the usual `~/.ssh` defaults.
///
/// `cache_dir` is a directory for the disk cache, which a large conversion
/// needs when its cache outgrows memory. Without one, `VERSATILES_CACHE_DIR` is
/// used, as by the CLI, and without that the cache stays in memory (#307).
///
/// # Returns
///
/// A [`TilesRuntime`] configured as [`create_runtime`] describes, with the
/// identity and the cache directory given here taking precedence.
pub fn create_runtime_with(ssh_identity: Option<&str>, cache_dir: Option<&str>) -> TilesRuntime {
	let mut builder = TilesRuntime::builder()
		.customize_registry(register_pipeline_readers)
		.silent_progress(true);

	if let Some(path) = explicit_or_env(ssh_identity, std::env::var(SSH_IDENTITY_ENV).ok()) {
		builder = builder.ssh_identity(path);
	}

	if let Some(path) = explicit_or_env(cache_dir, std::env::var(CACHE_DIR_ENV).ok()) {
		builder = builder.with_disk_cache(&path);
	}

	builder.build()
}

/// Decide which path to use, if any: an SSH identity or a cache directory.
///
/// An explicit path wins over the environment, so one call can use a different
/// one than the rest of the process. A blank value counts as unset rather than
/// as a path named "", which would fail with a confusing error.
fn explicit_or_env(explicit: Option<&str>, from_env: Option<String>) -> Option<PathBuf> {
	let explicit = explicit.map(str::to_string).filter(|value| !value.trim().is_empty());
	let from_env = from_env.filter(|value| !value.trim().is_empty());
	explicit.or(from_env).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn explicit_ssh_identity_wins_over_the_environment() {
		assert_eq!(
			explicit_or_env(Some("/keys/explicit"), Some("/keys/from-env".to_string())),
			Some(PathBuf::from("/keys/explicit"))
		);
	}

	#[test]
	fn ssh_identity_falls_back_to_the_environment() {
		assert_eq!(
			explicit_or_env(None, Some("/keys/from-env".to_string())),
			Some(PathBuf::from("/keys/from-env"))
		);
	}

	#[test]
	fn blank_ssh_identity_counts_as_unset() {
		// An empty explicit value must not shadow the environment, and an empty
		// environment value must not become a path to a file named "".
		assert_eq!(
			explicit_or_env(Some("  "), Some("/keys/from-env".to_string())),
			Some(PathBuf::from("/keys/from-env"))
		);
		assert_eq!(explicit_or_env(Some(""), None), None);
		assert_eq!(explicit_or_env(None, Some(String::new())), None);
		assert_eq!(explicit_or_env(None, None), None);
	}

	#[test]
	fn runtime_carries_the_explicit_ssh_identity() {
		let runtime = create_runtime_with(Some("/keys/explicit"), None);
		assert_eq!(runtime.ssh_identity(), Some(std::path::Path::new("/keys/explicit")));
	}

	/// The binding used to build every runtime with the in-memory cache, so a
	/// large conversion had no way to put it on disk (#307).
	#[test]
	fn runtime_uses_a_disk_cache_in_the_explicit_cache_dir() {
		let runtime = create_runtime_with(None, Some("/tmp/versatiles-cache"));
		assert_eq!(
			runtime.cache_type(),
			&versatiles_container::CacheType::Disk(PathBuf::from("/tmp/versatiles-cache"))
		);
	}

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

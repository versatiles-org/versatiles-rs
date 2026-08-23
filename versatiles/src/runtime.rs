use std::path::PathBuf;

use versatiles_container::{RuntimeBuilder, TilesRuntime};
use versatiles_pipeline::register_pipeline_readers;

/// A runtime builder with the pipeline readers registered and the
/// `VERSATILES_CACHE_DIR` and `VERSATILES_SSH_IDENTITY` environment variables
/// applied.
pub fn create_runtime_builder() -> RuntimeBuilder {
	let mut builder = TilesRuntime::builder().customize_registry(register_pipeline_readers);

	// Allow setting disk cache via environment variable
	if let Ok(cache_dir) = std::env::var("VERSATILES_CACHE_DIR") {
		builder = builder.with_disk_cache(&PathBuf::from(cache_dir));
	}

	// Allow setting SSH identity file via environment variable
	if let Ok(ssh_identity) = std::env::var("VERSATILES_SSH_IDENTITY") {
		builder = builder.ssh_identity(PathBuf::from(ssh_identity));
	}

	builder
}

/// The runtime every command runs under.
#[must_use]
pub fn create_runtime() -> TilesRuntime {
	create_runtime_builder().build()
}

/// The same runtime, with progress bars silenced.
#[must_use]
pub fn create_test_runtime() -> TilesRuntime {
	create_runtime_builder().silent_progress(true).build()
}

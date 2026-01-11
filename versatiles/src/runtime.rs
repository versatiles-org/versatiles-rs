use std::path::PathBuf;
use versatiles_container::{RuntimeBuilder, TilesRuntime};
use versatiles_pipeline::register_pipeline_readers;

pub fn create_runtime_builder() -> RuntimeBuilder {
	let mut builder = TilesRuntime::builder().customize_registry(register_pipeline_readers);

	// Allow setting disk cache via environment variable
	if let Ok(cache_dir) = std::env::var("VERSATILES_CACHE_DIR") {
		builder = builder.with_disk_cache(&PathBuf::from(cache_dir));
	}

	builder
}

pub fn create_runtime() -> TilesRuntime {
	create_runtime_builder().build()
}

pub fn create_test_runtime() -> TilesRuntime {
	create_runtime_builder().silent_progress(true).build()
}

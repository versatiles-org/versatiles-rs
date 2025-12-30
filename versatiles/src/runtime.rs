use versatiles_container::{RuntimeBuilder, TilesRuntime};
use versatiles_pipeline::register_pipeline_readers;

pub fn create_runtime_builder() -> RuntimeBuilder {
	TilesRuntime::builder().customize_registry(register_pipeline_readers)
}

pub fn create_runtime() -> TilesRuntime {
	create_runtime_builder().build()
}

pub fn create_test_runtime() -> TilesRuntime {
	create_runtime_builder().silent(true).build()
}

use crate::runtime::register_vpl_readers;
use std::sync::Arc;
use versatiles_container::TilesRuntime;

pub fn create_runtime_with_vpl() -> Arc<TilesRuntime> {
	Arc::new(
		TilesRuntime::builder()
			.customize_registry(|registry| {
				register_vpl_readers(registry);
			})
			.build(),
	)
}

pub fn create_test_runtime() -> Arc<TilesRuntime> {
	create_runtime_with_vpl()
}

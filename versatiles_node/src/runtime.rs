use std::sync::Arc;
use versatiles::runtime::register_vpl_readers;
use versatiles_container::TilesRuntime;

pub fn create_runtime() -> Arc<TilesRuntime> {
	Arc::new(
		TilesRuntime::builder()
			.customize_registry(|registry| {
				register_vpl_readers(registry);
			})
			.build(),
	)
}

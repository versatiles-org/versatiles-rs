use versatiles::pipeline::register_pipeline_readers;
use versatiles_container::TilesRuntime;

pub fn create_runtime() -> TilesRuntime {
	TilesRuntime::builder()
		.customize_registry(register_pipeline_readers)
		.silent()
		.build()
}

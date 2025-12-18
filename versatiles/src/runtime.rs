use versatiles_container::{RuntimeBuilder, TilesReaderTrait, TilesRuntime};

pub fn create_runtime_builder() -> RuntimeBuilder {
	TilesRuntime::builder().customize_registry(|registry| {
		registry.register_reader_file("vpl", |p, r| async move {
			Ok(Box::new(versatiles_pipeline::PipelineReader::open_path(&p, r).await?) as Box<dyn TilesReaderTrait>)
		});

		registry.register_reader_data("vpl", |p, r| async move {
			Ok(
				Box::new(versatiles_pipeline::PipelineReader::open_reader(p, &std::env::current_dir().unwrap(), r).await?)
					as Box<dyn TilesReaderTrait>,
			)
		});
	})
}

pub fn create_runtime() -> TilesRuntime {
	create_runtime_builder().build()
}

pub fn create_test_runtime() -> TilesRuntime {
	create_runtime_builder().build()
}

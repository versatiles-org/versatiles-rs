use versatiles_container::{ContainerRegistry, ProcessingConfig, TilesReaderTrait};

pub fn get_registry(config: ProcessingConfig) -> ContainerRegistry {
	let mut registry = ContainerRegistry::default();
	registry.register_reader_file("vpl", move |p| {
		let config = config.clone();
		async move {
			let config = config.clone();
			Ok(Box::new(versatiles_pipeline::PipelineReader::open_path(&p, config).await?) as Box<dyn TilesReaderTrait>)
		}
	});

	registry
}

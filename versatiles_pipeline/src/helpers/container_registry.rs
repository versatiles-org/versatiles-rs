use super::PipelineReader;
use versatiles_container::{ContainerRegistry, TilesReaderTrait};

pub fn register_pipeline_readers(registry: &mut ContainerRegistry) {
	registry.register_reader_file("vpl", |p, r| async move {
		Ok(Box::new(PipelineReader::open_path(&p, r).await?) as Box<dyn TilesReaderTrait>)
	});

	registry.register_reader_data("vpl", |p, r| async move {
		Ok(
			Box::new(PipelineReader::open_reader(p, &std::env::current_dir().unwrap(), r).await?)
				as Box<dyn TilesReaderTrait>,
		)
	});
}

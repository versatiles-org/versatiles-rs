use super::PipelineReader;
use std::sync::Arc;
use versatiles_container::{ContainerRegistry, TileSource};

pub fn register_pipeline_readers(registry: &mut ContainerRegistry) {
	registry.register_reader_file("vpl", |p, r| async move {
		Ok(Arc::new(
			Box::new(PipelineReader::open_path(&p, r).await?) as Box<dyn TileSource>
		))
	});

	registry.register_reader_data("vpl", |p, r| async move {
		Ok(Arc::new(
			Box::new(PipelineReader::open_reader(p, &std::env::current_dir().unwrap(), r).await?) as Box<dyn TileSource>,
		))
	});
}

use super::PipelineReader;
use versatiles_container::ContainerRegistry;

pub fn register_pipeline_readers(registry: &mut ContainerRegistry) {
	registry.register_reader::<PipelineReader>("vpl");
}

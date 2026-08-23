use versatiles_container::ContainerRegistry;

use super::PipelineReader;

/// Teaches a registry to open `.vpl` files as tile sources.
pub fn register_pipeline_readers(registry: &mut ContainerRegistry) {
	registry.register_reader::<PipelineReader>("vpl");
}

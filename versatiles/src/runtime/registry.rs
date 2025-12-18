/// This module provides a function to create a `ContainerRegistry` pre-configured
/// with specific file readers, such as `.vpl` for pipelines.
use versatiles_container::{TilesReaderTrait, TilesRuntime};

/// Registers additional readers (like `.vpl` for pipelines) to a `ContainerRegistry`.
///
/// # Parameters
/// - `registry`: A mutable reference to `ContainerRegistry` that will be configured with additional readers.
///
/// # Behavior
/// Registers an additional reader for the `.vpl` file extension.
pub fn register_vpl_readers(registry: &mut versatiles_container::ContainerRegistry) {
	// Register a reader for "vpl" files
	registry.register_reader_file("vpl", |p| async move {
		// We can't easily pass runtime here, so we create a default one
		let runtime = std::sync::Arc::new(TilesRuntime::default());
		Ok(Box::new(versatiles_pipeline::PipelineReader::open_path(&p, runtime).await?) as Box<dyn TilesReaderTrait>)
	});

	registry.register_reader_data("vpl", |p| async move {
		let runtime = std::sync::Arc::new(TilesRuntime::default());
		Ok(Box::new(
			versatiles_pipeline::PipelineReader::open_reader(p, &std::env::current_dir().unwrap(), runtime).await?,
		) as Box<dyn TilesReaderTrait>)
	});
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::Arc;

	#[tokio::test]
	async fn test_register_readers() {
		let runtime = Arc::new(
			TilesRuntime::builder()
				.customize_registry(|registry| {
					register_vpl_readers(registry);
				})
				.build(),
		);
		let reader_result: Result<Box<dyn TilesReaderTrait>, anyhow::Error> =
			runtime.registry().get_reader_from_str("test.vpl").await;
		assert!(reader_result.is_err(), "Expected error for non-existent file");
	}
}

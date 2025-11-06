/// This module provides a function to create a `ContainerRegistry` pre-configured
/// with specific file readers, such as `.vpl` for pipelines.
use versatiles_container::{ContainerRegistry, ProcessingConfig, TilesReaderTrait};

/// Creates a `ContainerRegistry` with pre-registered readers for specific file types.
///
/// # Parameters
/// - `config`: A `ProcessingConfig` instance used to configure the readers.
///
/// # Returns
/// A `ContainerRegistry` with readers registered for handling certain file extensions.
///
/// # Behavior
/// Registers an additional reader for the `.vpl` file extension.
///
/// # Example
/// ```rust
/// use versatiles::{
///    container::{ProcessingConfig, ContainerRegistry, TilesReaderTrait},
///    get_registry
/// };
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let registry: ContainerRegistry = get_registry(ProcessingConfig::default());
///     let reader = registry.get_reader("../testdata/berlin.vpl").await?;
///     // Use the reader here
///     Ok(())
/// }
/// ```
pub fn get_registry(config: ProcessingConfig) -> ContainerRegistry {
	let mut registry = ContainerRegistry::default();
	// Register a reader for "vpl" files. The closure captures the config and clones it for async usage.
	registry.register_reader_file("vpl", move |p| {
		let config = config.clone();
		async move {
			// Clone config again inside async block to ensure it is owned
			let config = config.clone();
			Ok(Box::new(versatiles_pipeline::PipelineReader::open_path(&p, config).await?) as Box<dyn TilesReaderTrait>)
		}
	});

	registry
}

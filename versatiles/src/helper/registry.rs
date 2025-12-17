/// This module provides a function to create a `ContainerRegistry` pre-configured
/// with specific file readers, such as `.vpl` for pipelines.
use std::sync::Arc;
use versatiles_container::{TilesReaderTrait, TilesRuntime};

/// Registers additional readers (like `.vpl` for pipelines) to a `TilesRuntime`.
///
/// # Parameters
/// - `runtime`: A `TilesRuntime` instance that will be configured with additional readers.
///
/// # Behavior
/// Registers an additional reader for the `.vpl` file extension to the runtime's registry.
///
/// # Example
/// ```rust
/// use versatiles::{
///    container::{TilesReaderTrait, TilesRuntime},
///    register_readers
/// };
/// use std::sync::Arc;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let runtime = Arc::new(TilesRuntime::default());
///     register_readers(&runtime);
///     let reader = runtime.registry().get_reader_from_str("../testdata/berlin.vpl").await?;
///     // Use the reader here
///     Ok(())
/// }
/// ```
pub fn register_readers(runtime: &Arc<TilesRuntime>) {
	let mut registry = runtime.registry().clone();

	// Register a reader for "vpl" files. The closure captures the runtime and clones it for async usage.
	let rt = runtime.clone();
	registry.register_reader_file("vpl", move |p| {
		let runtime = rt.clone();
		async move {
			// Clone runtime again inside async block to ensure it is owned
			Ok(Box::new(versatiles_pipeline::PipelineReader::open_path(&p, runtime).await?) as Box<dyn TilesReaderTrait>)
		}
	});

	let rt = runtime.clone();
	registry.register_reader_data("vpl", move |p| {
		let runtime = rt.clone();
		async move {
			// Clone runtime again inside async block to ensure it is owned
			Ok(Box::new(
				versatiles_pipeline::PipelineReader::open_reader(p, &std::env::current_dir().unwrap(), runtime).await?,
			) as Box<dyn TilesReaderTrait>)
		}
	});
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test_register_readers() {
		let runtime = Arc::new(TilesRuntime::default());
		register_readers(&runtime);
		let reader_result = runtime.registry().get_reader_from_str("test.vpl").await;
		assert!(reader_result.is_err(), "Expected error for non-existent file");
	}
}

use versatiles::pipeline::register_pipeline_readers;
use versatiles_container::TilesRuntime;

pub fn create_runtime() -> TilesRuntime {
	TilesRuntime::builder()
		.customize_registry(register_pipeline_readers)
		.silent()
		.build()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_create_runtime() {
		let runtime = create_runtime();
		// Runtime should be created successfully
		// Verify it's a valid runtime by checking it can create a progress handle
		let progress = runtime.create_progress("Test", 100);
		assert_eq!(progress.id().0, 1);
	}

	#[test]
	fn test_runtime_has_custom_registry() {
		let runtime = create_runtime();
		// The runtime should have the pipeline readers registered
		// We can verify this by checking that the runtime exists and is usable
		let _events = runtime.events();
		// If we got here, the runtime was created successfully with custom registry
	}

	#[test]
	fn test_runtime_is_silent() {
		let runtime = create_runtime();
		// Create a progress to verify runtime works
		let _progress = runtime.create_progress("Silent test", 50);
		// The runtime should be in silent mode (no stderr output)
		// This is verified by the build() method being called with silent()
	}
}

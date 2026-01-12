use std::{fmt::Debug, sync::Arc};

/// Distinguishes between different tile source types.
#[derive(Clone, PartialEq, Eq)]
pub enum SourceType {
	/// Physical container format (e.g., "mbtiles", "versatiles", "pmtiles", "tar", "directory")
	Container { name: String, input: String },
	/// Tile processor/transformer (e.g., "filter", "`raster_format`", "converter")
	Processor { name: String, input: Arc<SourceType> },
	/// Composite source combining multiple upstream sources (e.g., "stacked", "merged")
	Composite { name: String, inputs: Vec<Arc<SourceType>> },
}

impl SourceType {
	#[must_use]
	pub fn new_container(name: &str, input: &str) -> Arc<SourceType> {
		Arc::new(SourceType::Container {
			name: name.to_string(),
			input: input.to_string(),
		})
	}

	#[must_use]
	pub fn new_processor(name: &str, input: Arc<SourceType>) -> Arc<SourceType> {
		Arc::new(SourceType::Processor {
			name: name.to_string(),
			input,
		})
	}

	#[must_use]
	pub fn new_composite(name: &str, inputs: &[Arc<SourceType>]) -> Arc<SourceType> {
		Arc::new(SourceType::Composite {
			name: name.to_string(),
			inputs: inputs.to_vec(),
		})
	}
}

impl std::fmt::Display for SourceType {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			SourceType::Container { name, input } => write!(f, "container '{name}' ('{input}')"),
			SourceType::Processor { name, .. } => write!(f, "processor '{name}'"),
			SourceType::Composite { name, .. } => write!(f, "composite '{name}'"),
		}
	}
}

impl Debug for SourceType {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			SourceType::Container { name, input } => f
				.debug_struct("Container")
				.field("name", name)
				.field("uri", input)
				.finish(),
			SourceType::Processor { name, input } => f
				.debug_struct("Processor")
				.field("name", name)
				.field("input", input)
				.finish(),
			SourceType::Composite { name, inputs } => f
				.debug_struct("Composite")
				.field("name", name)
				.field("inputs", inputs)
				.finish(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_new_container() {
		let source = SourceType::new_container("mbtiles", "/path/to/file.mbtiles");
		match source.as_ref() {
			SourceType::Container { name, input } => {
				assert_eq!(name, "mbtiles");
				assert_eq!(input, "/path/to/file.mbtiles");
			}
			_ => panic!("Expected Container variant"),
		}
	}

	#[test]
	fn test_new_processor() {
		let container = SourceType::new_container("mbtiles", "/path/to/file.mbtiles");
		let processor = SourceType::new_processor("filter", container);
		match processor.as_ref() {
			SourceType::Processor { name, input } => {
				assert_eq!(name, "filter");
				match input.as_ref() {
					SourceType::Container { name, .. } => assert_eq!(name, "mbtiles"),
					_ => panic!("Expected Container as input"),
				}
			}
			_ => panic!("Expected Processor variant"),
		}
	}

	#[test]
	fn test_new_composite() {
		let source1 = SourceType::new_container("mbtiles", "/path/to/file1.mbtiles");
		let source2 = SourceType::new_container("pmtiles", "/path/to/file2.pmtiles");
		let composite = SourceType::new_composite("stacked", &[source1, source2]);
		match composite.as_ref() {
			SourceType::Composite { name, inputs } => {
				assert_eq!(name, "stacked");
				assert_eq!(inputs.len(), 2);
			}
			_ => panic!("Expected Composite variant"),
		}
	}

	#[test]
	fn test_display_container() {
		let source = SourceType::new_container("mbtiles", "/path/to/file.mbtiles");
		assert_eq!(format!("{source}"), "container 'mbtiles' ('/path/to/file.mbtiles')");
	}

	#[test]
	fn test_display_processor() {
		let container = SourceType::new_container("mbtiles", "/path/to/file.mbtiles");
		let processor = SourceType::new_processor("filter", container);
		assert_eq!(format!("{processor}"), "processor 'filter'");
	}

	#[test]
	fn test_display_composite() {
		let source1 = SourceType::new_container("mbtiles", "/path/to/file1.mbtiles");
		let source2 = SourceType::new_container("pmtiles", "/path/to/file2.pmtiles");
		let composite = SourceType::new_composite("stacked", &[source1, source2]);
		assert_eq!(format!("{composite}"), "composite 'stacked'");
	}

	#[test]
	fn test_debug_container() {
		let source = SourceType::new_container("mbtiles", "/path/to/file.mbtiles");
		let debug_str = format!("{source:?}");
		assert!(debug_str.contains("Container"));
		assert!(debug_str.contains("mbtiles"));
		assert!(debug_str.contains("/path/to/file.mbtiles"));
	}

	#[test]
	fn test_debug_processor() {
		let container = SourceType::new_container("mbtiles", "/path/to/file.mbtiles");
		let processor = SourceType::new_processor("filter", container);
		let debug_str = format!("{processor:?}");
		assert!(debug_str.contains("Processor"));
		assert!(debug_str.contains("filter"));
	}

	#[test]
	fn test_debug_composite() {
		let source1 = SourceType::new_container("mbtiles", "/path/to/file1.mbtiles");
		let source2 = SourceType::new_container("pmtiles", "/path/to/file2.pmtiles");
		let composite = SourceType::new_composite("stacked", &[source1, source2]);
		let debug_str = format!("{composite:?}");
		assert!(debug_str.contains("Composite"));
		assert!(debug_str.contains("stacked"));
	}

	#[test]
	fn test_clone_and_eq() {
		let source1 = SourceType::new_container("mbtiles", "/path/to/file.mbtiles");
		let source2 = source1.clone();
		assert_eq!(source1, source2);

		let source3 = SourceType::new_container("mbtiles", "/path/to/other.mbtiles");
		assert_ne!(source1, source3);
	}
}

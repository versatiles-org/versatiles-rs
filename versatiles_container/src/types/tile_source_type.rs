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

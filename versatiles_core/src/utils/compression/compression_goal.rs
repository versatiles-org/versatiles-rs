use std::fmt::{self, Debug};

/// Defines the desired compression objective.
#[derive(Clone, Copy, PartialEq)]
pub enum CompressionGoal {
	/// Prioritize speed over compression ratio.
	UseFastCompression,
	/// Prioritize compression ratio over speed.
	UseBestCompression,
	/// Treat data as incompressible.
	IsIncompressible,
}

impl Debug for CompressionGoal {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::UseFastCompression => write!(f, "Use Fast Compression"),
			Self::UseBestCompression => write!(f, "Use Best Compression"),
			Self::IsIncompressible => write!(f, "Is Incompressible"),
		}
	}
}

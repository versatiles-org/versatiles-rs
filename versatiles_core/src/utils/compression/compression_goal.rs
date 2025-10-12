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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_debug() {
		use CompressionGoal::*;
		assert_eq!(format!("{UseFastCompression:?}"), "Use Fast Compression");
		assert_eq!(format!("{UseBestCompression:?}"), "Use Best Compression");
		assert_eq!(format!("{IsIncompressible:?}"), "Is Incompressible");
	}

	#[test]
	fn test_clone_copy_and_eq() {
		let a = CompressionGoal::UseBestCompression;
		let b = a;
		assert_eq!(a, b);
	}
}

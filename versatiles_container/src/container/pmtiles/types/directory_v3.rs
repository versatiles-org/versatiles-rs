use std::fmt::Debug;
use versatiles_core::Blob;

pub struct Directory {
	pub root_bytes: Blob,
	pub leaves_bytes: Blob,
}

impl Debug for Directory {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Directory")
			.field("root_bytes", &self.root_bytes.len())
			.field("leaves_bytes", &self.leaves_bytes.len())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_debug() {
		let dir = Directory {
			root_bytes: Blob::from(vec![1, 2, 3]),
			leaves_bytes: Blob::from(vec![4, 5]),
		};
		let debug_str = format!("{:?}", dir);
		assert!(debug_str.contains("root_bytes: 3"));
		assert!(debug_str.contains("leaves_bytes: 2"));
	}
}

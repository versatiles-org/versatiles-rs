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

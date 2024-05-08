use versatiles_lib::shared::{Blob, Compression};

pub struct SourceResponse {
	pub blob: Blob,
	pub compression: Compression,
	pub mime: String,
}

impl SourceResponse {
	pub fn new_some(blob: Blob, compression: &Compression, mime: &str) -> Option<SourceResponse> {
		Some(SourceResponse {
			blob,
			compression: compression.to_owned(),
			mime: mime.to_owned(),
		})
	}
}

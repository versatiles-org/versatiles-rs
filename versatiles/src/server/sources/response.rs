use versatiles_core::{Blob, TileCompression};

pub struct SourceResponse {
	pub blob: Blob,
	pub compression: TileCompression,
	pub mime: String,
}

impl SourceResponse {
	pub fn new_some(blob: Blob, compression: TileCompression, mime: &str) -> Option<SourceResponse> {
		Some(SourceResponse {
			blob,
			compression: compression.to_owned(),
			mime: mime.to_owned(),
		})
	}
}

use versatiles_core::{Blob, TileCompression};

pub struct SourceResponse {
	pub blob: Blob,
	pub compression: TileCompression,
	pub mime: String,
}

/// What a tile source has to say about one request.
pub enum TileResponse {
	/// The tile, or the source's TileJSON.
	Data(SourceResponse),
	/// A vector tile inside the zoom range the source covers that holds no
	/// data: open sea, or outside the covered area. Answered with `204`.
	Empty,
	/// Nothing this source has: a raster tile without data, a zoom level
	/// outside the range, coordinates that cannot exist, a path that is not a
	/// tile. Answered with `404`.
	NotFound,
}

impl SourceResponse {
	pub fn new_some(blob: Blob, compression: TileCompression, mime: &str) -> Option<SourceResponse> {
		Some(SourceResponse {
			blob,
			compression,
			mime: mime.to_owned(),
		})
	}
}

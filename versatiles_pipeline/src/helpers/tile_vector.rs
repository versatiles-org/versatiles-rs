use anyhow::Result;
use versatiles_core::{
	utils::{compress, decompress},
	{Blob, TileStream, TilesReaderParameters},
};
use versatiles_geometry::vector_tile::VectorTile;

#[allow(dead_code)]
pub fn unpack_vector_tile(
	blob: Result<Option<Blob>>,
	parameters: &TilesReaderParameters,
) -> Result<Option<VectorTile>> {
	blob?
		.map(|blob| VectorTile::from_blob(&decompress(blob, &parameters.tile_compression)?))
		.transpose()
}

pub fn unpack_vector_tile_stream<'a>(
	stream: Result<TileStream<'a>>,
	parameters: &TilesReaderParameters,
) -> Result<TileStream<'a, VectorTile>> {
	let tile_compression = parameters.tile_compression;
	Ok(stream?.map_item_parallel(move |blob| VectorTile::from_blob(&decompress(blob, &tile_compression)?)))
}

#[allow(dead_code)]
pub fn pack_vector_tile(tile: Result<Option<VectorTile>>, parameters: &TilesReaderParameters) -> Result<Option<Blob>> {
	tile?
		.map(|vector_tile| compress(vector_tile.to_blob()?, &parameters.tile_compression))
		.transpose()
}

pub fn pack_vector_tile_stream<'a>(
	stream: Result<TileStream<'a, VectorTile>>,
	parameters: &TilesReaderParameters,
) -> Result<TileStream<'a, Blob>> {
	let tile_compression = parameters.tile_compression;
	Ok(stream?.map_item_parallel(move |vector_tile| compress(vector_tile.to_blob()?, &tile_compression)))
}

#[cfg(test)]
mod tests {
	use std::vec;

	use super::*;
	use lazy_static::lazy_static;
	use versatiles_core::{
		TileCompression::{self, Gzip},
		TileCoord,
		TileFormat::{self, MVT},
	};
	use versatiles_geometry::vector_tile::VectorTileLayer;

	lazy_static! {
		static ref TEST_TILE: VectorTile = VectorTile::new(vec![VectorTileLayer::new("test_layer".to_string(), 8192, 3)]);
		static ref TEST_BLOB: Blob = TEST_TILE.to_blob().unwrap();
		static ref TEST_COMPRESSED_BLOB: Blob = compress(TEST_BLOB.clone(), &Gzip).unwrap();
	}

	fn parameters(format: TileFormat, compression: TileCompression) -> TilesReaderParameters {
		TilesReaderParameters {
			tile_format: format,
			tile_compression: compression,
			..Default::default()
		}
	}

	#[test]
	fn test_unpack_vector_tile() {
		let result = unpack_vector_tile(Ok(Some(TEST_COMPRESSED_BLOB.clone())), &parameters(MVT, Gzip))
			.unwrap()
			.unwrap();
		assert_eq!(result, TEST_TILE.clone());
	}

	#[tokio::test]
	async fn test_unpack_vector_tile_stream() {
		let tile_stream = TileStream::from_vec(vec![(TileCoord::new(0, 0, 0).unwrap(), TEST_COMPRESSED_BLOB.clone())]);
		let result = unpack_vector_tile_stream(Ok(tile_stream), &parameters(MVT, Gzip)).unwrap();
		let vec = result.to_vec().await;
		assert_eq!(vec.len(), 1);
		assert_eq!(vec[0].1, TEST_TILE.clone());
	}

	#[test]
	fn test_pack_vector_tile() {
		let result = pack_vector_tile(Ok(Some(TEST_TILE.clone())), &parameters(MVT, Gzip))
			.unwrap()
			.unwrap();
		assert_eq!(result, TEST_COMPRESSED_BLOB.clone());
	}

	#[tokio::test]
	async fn test_pack_vector_tile_stream() {
		let tile_stream = TileStream::from_vec(vec![(TileCoord::new(0, 0, 0).unwrap(), TEST_TILE.clone())]);
		let result = pack_vector_tile_stream(Ok(tile_stream), &parameters(MVT, Gzip)).unwrap();
		let vec = result.to_vec().await;
		assert_eq!(vec.len(), 1);
		assert_eq!(vec[0].1, TEST_COMPRESSED_BLOB.clone());
	}
}

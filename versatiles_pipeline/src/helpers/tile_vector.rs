use anyhow::Result;
use versatiles_core::{
	types::{Blob, TileCompression, TileFormat, TileStream},
	utils::{compress, decompress},
};
use versatiles_geometry::vector_tile::VectorTile;

pub fn unpack_vector_tile(
	tile_data: Result<Option<Blob>>,
	_tile_format: TileFormat,
	tile_compression: TileCompression,
) -> Result<Option<VectorTile>> {
	tile_data?
		.map(|blob| VectorTile::from_blob(&decompress(blob, &tile_compression)?))
		.transpose()
}

pub fn unpack_vector_tile_stream(
	tile_stream: Result<TileStream>,
	_tile_format: TileFormat,
	tile_compression: TileCompression,
) -> Result<TileStream<VectorTile>> {
	Ok(tile_stream?.map_item_parallel(move |blob| VectorTile::from_blob(&decompress(blob, &tile_compression)?)))
}

pub fn pack_vector_tile(
	vector_tile: Result<Option<VectorTile>>,
	_tile_format: TileFormat,
	tile_compression: TileCompression,
) -> Result<Option<Blob>> {
	vector_tile?
		.map(|vector_tile| compress(vector_tile.to_blob()?, &tile_compression))
		.transpose()
}

pub fn pack_vector_tile_stream(
	tile_stream: Result<TileStream<VectorTile>>,
	_tile_format: TileFormat,
	tile_compression: TileCompression,
) -> Result<TileStream<Blob>> {
	Ok(tile_stream?.map_item_parallel(move |vector_tile| compress(vector_tile.to_blob()?, &tile_compression)))
}

#[cfg(test)]
mod tests {
	use std::vec;

	use super::*;
	use lazy_static::lazy_static;
	use versatiles_core::types::{TileCompression, TileCoord3, TileFormat};
	use versatiles_geometry::vector_tile::VectorTileLayer;

	lazy_static! {
		static ref TEST_TILE: VectorTile = VectorTile::new(vec![VectorTileLayer::new("test_layer".to_string(), 8192, 3)]);
		static ref TEST_BLOB: Blob = TEST_TILE.to_blob().unwrap();
		static ref TEST_COMPRESSED_BLOB: Blob = compress(TEST_BLOB.clone(), &TileCompression::Gzip).unwrap();
	}

	#[test]
	fn test_unpack_vector_tile() {
		let result = unpack_vector_tile(
			Ok(Some(TEST_COMPRESSED_BLOB.clone())),
			TileFormat::MVT,
			TileCompression::Gzip,
		)
		.unwrap()
		.unwrap();
		assert_eq!(result, TEST_TILE.clone());
	}

	#[tokio::test]
	async fn test_unpack_vector_tile_stream() {
		let tile_stream = TileStream::from_vec(vec![(TileCoord3::new(0, 0, 0).unwrap(), TEST_COMPRESSED_BLOB.clone())]);
		let result = unpack_vector_tile_stream(Ok(tile_stream), TileFormat::MVT, TileCompression::Gzip).unwrap();
		let vec = result.collect().await;
		assert_eq!(vec.len(), 1);
		assert_eq!(vec[0].1, TEST_TILE.clone());
	}

	#[test]
	fn test_pack_vector_tile() {
		let result = pack_vector_tile(Ok(Some(TEST_TILE.clone())), TileFormat::MVT, TileCompression::Gzip)
			.unwrap()
			.unwrap();
		assert_eq!(result, TEST_COMPRESSED_BLOB.clone());
	}

	#[tokio::test]
	async fn test_pack_vector_tile_stream() {
		let tile_stream = TileStream::from_vec(vec![(TileCoord3::new(0, 0, 0).unwrap(), TEST_TILE.clone())]);
		let result = pack_vector_tile_stream(Ok(tile_stream), TileFormat::MVT, TileCompression::Gzip).unwrap();
		let vec = result.collect().await;
		assert_eq!(vec.len(), 1);
		assert_eq!(vec[0].1, TEST_COMPRESSED_BLOB.clone());
	}
}

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
	Ok(tile_stream?.map_item_parallel(move |blob| Ok(VectorTile::from_blob(&decompress(blob, &tile_compression)?)?)))
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

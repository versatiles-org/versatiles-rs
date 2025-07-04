use anyhow::Result;
use imageproc::image::DynamicImage;
use versatiles_core::{
	types::{Blob, TileCompression, TileFormat, TileStream},
	utils::{compress, decompress},
};
use versatiles_image::{blob2image, image2blob};

pub fn unpack_image_tile(
	tile_data: Result<Option<Blob>>,
	tile_format: TileFormat,
	tile_compression: TileCompression,
) -> Result<Option<DynamicImage>> {
	tile_data?
		.map(|blob| blob2image(&decompress(blob, &tile_compression)?, tile_format))
		.transpose()
}

pub fn unpack_image_tile_stream(
	tile_stream: Result<TileStream>,
	tile_format: TileFormat,
	tile_compression: TileCompression,
) -> Result<TileStream<DynamicImage>> {
	Ok(tile_stream?.map_item_parallel(move |blob| Ok(blob2image(&decompress(blob, &tile_compression)?, tile_format)?)))
}

pub fn pack_image_tile(
	tile_image: Result<Option<DynamicImage>>,
	tile_format: TileFormat,
	tile_compression: TileCompression,
) -> Result<Option<Blob>> {
	tile_image?
		.map(|image| compress(image2blob(&image, tile_format)?, &tile_compression))
		.transpose()
}

pub fn pack_image_tile_stream(
	tile_stream: Result<TileStream<DynamicImage>>,
	tile_format: TileFormat,
	tile_compression: TileCompression,
) -> Result<TileStream<Blob>> {
	Ok(tile_stream?.map_item_parallel(move |image| compress(image2blob(&image, tile_format)?, &tile_compression)))
}

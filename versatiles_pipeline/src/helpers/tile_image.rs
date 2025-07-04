use anyhow::Result;
use imageproc::image::DynamicImage;
use versatiles_core::{
	types::{Blob, TileCompression, TileFormat, TileStream},
	utils::{compress, decompress},
};
use versatiles_image::EnhancedDynamicImageTrait;

pub fn unpack_image_tile(
	tile_data: Result<Option<Blob>>,
	tile_format: TileFormat,
	tile_compression: TileCompression,
) -> Result<Option<DynamicImage>> {
	tile_data?
		.map(|blob| DynamicImage::from_blob(&decompress(blob, &tile_compression)?, tile_format))
		.transpose()
}

pub fn unpack_image_tile_stream(
	tile_stream: Result<TileStream>,
	tile_format: TileFormat,
	tile_compression: TileCompression,
) -> Result<TileStream<DynamicImage>> {
	Ok(tile_stream?
		.map_item_parallel(move |blob| DynamicImage::from_blob(&decompress(blob, &tile_compression)?, tile_format)))
}

pub fn pack_image_tile(
	tile_image: Result<Option<DynamicImage>>,
	tile_format: TileFormat,
	tile_compression: TileCompression,
) -> Result<Option<Blob>> {
	tile_image?
		.map(|image| compress(image.to_blob(tile_format)?, &tile_compression))
		.transpose()
}

pub fn pack_image_tile_stream(
	tile_stream: Result<TileStream<DynamicImage>>,
	tile_format: TileFormat,
	tile_compression: TileCompression,
) -> Result<TileStream<Blob>> {
	Ok(tile_stream?.map_item_parallel(move |image| compress(image.to_blob(tile_format)?, &tile_compression)))
}

#[cfg(test)]
mod tests {
	use super::*;
	use lazy_static::lazy_static;
	use versatiles_core::types::TileCoord3;

	lazy_static! {
		static ref TEST_IMAGE: DynamicImage = DynamicImage::new_rgb8(64, 64);
		static ref TEST_BLOB: Blob = TEST_IMAGE.to_blob(TileFormat::PNG).unwrap();
	}

	#[test]
	fn test_unpack_image_tile() {
		let compressed_blob = compress(TEST_BLOB.clone(), &TileCompression::Gzip).unwrap();
		let result = unpack_image_tile(Ok(Some(compressed_blob)), TileFormat::PNG, TileCompression::Gzip).unwrap();
		assert!(result.is_some());
		result.unwrap().ensure_same_meta(&TEST_IMAGE).unwrap();
	}

	#[tokio::test]
	async fn test_unpack_image_tile_stream() {
		let compressed_blob = compress(TEST_BLOB.clone(), &TileCompression::Gzip).unwrap();
		let tile_stream = TileStream::from_vec(vec![(TileCoord3::new(0, 0, 0).unwrap(), compressed_blob)]);
		let result = unpack_image_tile_stream(Ok(tile_stream), TileFormat::PNG, TileCompression::Gzip).unwrap();
		let images: Vec<_> = result.collect().await;
		assert_eq!(images.len(), 1);
		images[0].1.ensure_same_meta(&TEST_IMAGE).unwrap();
	}

	#[test]
	fn test_pack_image_tile() {
		let result = pack_image_tile(Ok(Some(TEST_IMAGE.clone())), TileFormat::PNG, TileCompression::Gzip).unwrap();
		assert!(result.is_some());
		let decompressed_blob = decompress(result.unwrap(), &TileCompression::Gzip).unwrap();
		let unpacked_image = DynamicImage::from_blob(&decompressed_blob, TileFormat::PNG).unwrap();
		unpacked_image.ensure_same_meta(&TEST_IMAGE).unwrap();
	}

	#[tokio::test]
	async fn test_pack_image_tile_stream() {
		let tile_stream = TileStream::from_vec(vec![(TileCoord3::new(0, 0, 0).unwrap(), TEST_IMAGE.clone())]);
		let result = pack_image_tile_stream(Ok(tile_stream), TileFormat::PNG, TileCompression::Gzip).unwrap();
		let blobs: Vec<_> = result.collect().await;
		assert_eq!(blobs.len(), 1);
		let decompressed_blob = decompress(blobs[0].1.clone(), &TileCompression::Gzip).unwrap();
		let unpacked_image = DynamicImage::from_blob(&decompressed_blob, TileFormat::PNG).unwrap();
		unpacked_image.ensure_same_meta(&TEST_IMAGE).unwrap();
	}
}

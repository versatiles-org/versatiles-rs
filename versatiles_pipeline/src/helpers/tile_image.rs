use anyhow::Result;
use imageproc::image::DynamicImage;
use versatiles_core::{
	utils::{compress, decompress},
	*,
};
use versatiles_image::traits::*;

#[allow(dead_code)]
pub fn unpack_image_tile(
	blob: Result<Option<Blob>>,
	parameters: &TilesReaderParameters,
) -> Result<Option<DynamicImage>> {
	blob?
		.map(|blob| DynamicImage::from_blob(&decompress(blob, &parameters.tile_compression)?, parameters.tile_format))
		.transpose()
}

pub fn unpack_image_tile_stream<'a>(
	stream: Result<TileStream<'a>>,
	parameters: &TilesReaderParameters,
) -> Result<TileStream<'a, DynamicImage>> {
	let tile_compression = parameters.tile_compression;
	let tile_format = parameters.tile_format;
	Ok(stream?
		.map_item_parallel(move |blob| DynamicImage::from_blob(&decompress(blob, &tile_compression)?, tile_format)))
}

#[allow(dead_code)]
pub fn pack_image_tile(
	image: Result<Option<DynamicImage>>,
	parameters: &TilesReaderParameters,
) -> Result<Option<Blob>> {
	image?
		.map(|image| compress(image.to_blob(parameters.tile_format)?, &parameters.tile_compression))
		.transpose()
}

pub fn pack_image_tile_stream<'a>(
	stream: Result<TileStream<'a, DynamicImage>>,
	parameters: &TilesReaderParameters,
) -> Result<TileStream<'a, Blob>> {
	let tile_compression = parameters.tile_compression;
	let tile_format = parameters.tile_format;
	Ok(stream?.map_item_parallel(move |image| compress(image.to_blob(tile_format)?, &tile_compression)))
}

#[cfg(test)]
mod tests {
	use super::*;
	use lazy_static::lazy_static;
	use versatiles_core::{
		TileCompression::{self, Gzip},
		TileCoord3,
		TileFormat::{self, PNG},
	};

	lazy_static! {
		static ref TEST_IMAGE: DynamicImage = DynamicImage::new_rgb8(64, 64);
		static ref TEST_BLOB: Blob = TEST_IMAGE.to_blob(PNG).unwrap();
	}

	fn parameters(format: TileFormat, compression: TileCompression) -> TilesReaderParameters {
		TilesReaderParameters {
			tile_format: format,
			tile_compression: compression,
			..Default::default()
		}
	}

	#[test]
	fn test_unpack_image_tile() {
		let compressed_blob = compress(TEST_BLOB.clone(), &Gzip).unwrap();
		let result = unpack_image_tile(Ok(Some(compressed_blob)), &parameters(PNG, Gzip)).unwrap();
		assert!(result.is_some());
		result.unwrap().ensure_same_meta(&TEST_IMAGE).unwrap();
	}

	#[tokio::test]
	async fn test_unpack_image_tile_stream() {
		let compressed_blob = compress(TEST_BLOB.clone(), &Gzip).unwrap();
		let tile_stream = TileStream::from_vec(vec![(TileCoord3::new(0, 0, 0).unwrap(), compressed_blob)]);
		let result = unpack_image_tile_stream(Ok(tile_stream), &parameters(PNG, Gzip)).unwrap();
		let images: Vec<_> = result.to_vec().await;
		assert_eq!(images.len(), 1);
		images[0].1.ensure_same_meta(&TEST_IMAGE).unwrap();
	}

	#[test]
	fn test_pack_image_tile() {
		let result = pack_image_tile(Ok(Some(TEST_IMAGE.clone())), &parameters(PNG, Gzip)).unwrap();
		assert!(result.is_some());
		let decompressed_blob = decompress(result.unwrap(), &Gzip).unwrap();
		let unpacked_image = DynamicImage::from_blob(&decompressed_blob, PNG).unwrap();
		unpacked_image.ensure_same_meta(&TEST_IMAGE).unwrap();
	}

	#[tokio::test]
	async fn test_pack_image_tile_stream() {
		let tile_stream = TileStream::from_vec(vec![(TileCoord3::new(0, 0, 0).unwrap(), TEST_IMAGE.clone())]);
		let result = pack_image_tile_stream(Ok(tile_stream), &parameters(PNG, Gzip)).unwrap();
		let blobs: Vec<_> = result.to_vec().await;
		assert_eq!(blobs.len(), 1);
		let decompressed_blob = decompress(blobs[0].1.clone(), &Gzip).unwrap();
		let unpacked_image = DynamicImage::from_blob(&decompressed_blob, PNG).unwrap();
		unpacked_image.ensure_same_meta(&TEST_IMAGE).unwrap();
	}
}

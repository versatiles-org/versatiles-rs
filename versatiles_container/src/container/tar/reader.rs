//! Provides functionality for reading tile data from a tar archive.

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use std::{collections::HashMap, fmt::Debug, io::Read, path::Path};
use tar::{Archive, EntryType};
use versatiles_core::{io::*, tilejson::TileJSON, utils::decompress, *};

/// A struct that provides functionality to read tile data from a tar archive.
pub struct TarTilesReader {
	tilejson: TileJSON,
	name: String,
	reader: Box<DataReaderFile>,
	tile_map: HashMap<TileCoord, ByteRange>,
	parameters: TilesReaderParameters,
}

impl TarTilesReader {
	/// Creates a new `TarTilesReader` from a given file path.
	///
	/// # Arguments
	/// * `path` - The path to the tar archive file.
	///
	/// # Errors
	/// Returns an error if the file cannot be opened or read.
	pub fn open_path(path: &Path) -> Result<TarTilesReader> {
		let mut reader = DataReaderFile::open(path)?;
		let mut archive = Archive::new(&mut reader);

		let mut tilejson = TileJSON::default();
		let mut tile_map = HashMap::new();
		let mut tile_format: Option<TileFormat> = None;
		let mut tile_compression: Option<TileCompression> = None;
		let mut bbox_pyramid = TileBBoxPyramid::new_empty();

		for entry in archive.entries()? {
			let mut entry = entry?;
			let header = entry.header();
			if header.entry_type() != EntryType::Regular {
				continue;
			}

			let path = entry.path()?.clone();
			let mut path_tmp: Vec<&str> = path.iter().map(|s| s.to_str().unwrap()).collect();

			if path_tmp[0] == "." {
				path_tmp.remove(0);
			}

			let path_tmp_string = path_tmp.join("/");
			drop(path);
			let path_vec: Vec<&str> = path_tmp_string.split('/').collect();

			if path_vec.len() == 3 {
				let level = path_vec[0].parse::<u8>()?;
				let x = path_vec[1].parse::<u32>()?;

				let mut filename: String = String::from(path_vec[2]);
				let this_compression = TileCompression::from_filename(&mut filename);
				let this_format = TileFormat::from_filename(&mut filename);

				if this_format.is_none() {
					continue;
				}
				let this_format = this_format.unwrap();

				let y = filename.parse::<u32>()?;

				if tile_format.is_none() {
					tile_format = Some(this_format);
				} else if tile_format.as_ref().unwrap() != &this_format {
					bail!("unknown filename {path_tmp_string:?}, can't detect format");
				}

				if tile_compression.is_none() {
					tile_compression = Some(this_compression);
				} else if tile_compression.as_ref().unwrap() != &this_compression {
					bail!("unknown filename {path_tmp_string:?}, can't detect compression");
				}

				let offset = entry.raw_file_position();
				let length = entry.size();

				let coord = TileCoord::new(level, x, y)?;
				bbox_pyramid.include_coord(&coord);
				tile_map.insert(coord, ByteRange { offset, length });
				continue;
			}

			let mut read_to_end = || {
				let mut blob: Vec<u8> = Vec::new();
				entry.read_to_end(&mut blob).unwrap();
				Blob::from(blob)
			};

			if path_vec.len() == 1 {
				match path_vec[0] {
					"meta.json" | "tiles.json" | "metadata.json" => {
						tilejson.merge(&TileJSON::try_from_blob_or_default(&read_to_end()))?;
						continue;
					}
					"meta.json.gz" | "tiles.json.gz" | "metadata.json.gz" => {
						tilejson.merge(&TileJSON::try_from_blob_or_default(&decompress(
							read_to_end(),
							&TileCompression::Gzip,
						)?))?;
						continue;
					}
					"meta.json.br" | "tiles.json.br" | "metadata.json.br" => {
						tilejson.merge(&TileJSON::try_from_blob_or_default(&decompress(
							read_to_end(),
							&TileCompression::Brotli,
						)?))?;
						continue;
					}
					&_ => {}
				};
			}

			log::warn!("unknown file in tar: {path_tmp_string:?}");
		}

		if tile_map.is_empty() {
			return Err(anyhow!("no tiles found in tar"));
		}

		let parameters = TilesReaderParameters::new(
			tile_format.ok_or(anyhow!("unknown tile format, can't detect format"))?,
			tile_compression.ok_or(anyhow!("unknown tile compression, can't detect compression"))?,
			bbox_pyramid.clone(),
		);

		Ok(TarTilesReader {
			tilejson,
			name: path.to_str().unwrap().to_string(),
			parameters,
			reader,
			tile_map,
		})
	}
}

#[async_trait]
impl TilesReaderTrait for TarTilesReader {
	/// Returns the container name.
	fn container_name(&self) -> &str {
		"tar"
	}

	/// Returns the parameters of the tiles reader.
	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	/// Overrides the tile compression method.
	///
	/// # Arguments
	/// * `tile_compression` - The new tile compression method.
	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.parameters.tile_compression = tile_compression;
	}

	/// Returns the metadata as a `Blob`.
	///
	/// # Errors
	/// Returns an error if there is an issue retrieving the metadata.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	/// Returns the tile data for the specified coordinates as a `Blob`.
	///
	/// # Arguments
	/// * `coord` - The coordinates of the tile.
	///
	/// # Errors
	/// Returns an error if there is an issue retrieving the tile data.
	async fn get_tile_blob(&self, coord: &TileCoord) -> Result<Option<Blob>> {
		log::trace!("get_tile_data {:?}", coord);

		let range = self.tile_map.get(coord);

		if let Some(range) = range {
			let blob = self.reader.read_range(range).await?;
			Ok(Some(blob))
		} else {
			Ok(None)
		}
	}

	/// Returns the name of the tar archive.
	fn source_name(&self) -> &str {
		&self.name
	}
}

impl Debug for TarTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TarTilesReader")
			.field("parameters", &self.parameters())
			.finish()
	}
}

#[cfg(test)]
pub mod tests {
	use super::*;
	use crate::{MOCK_BYTES_PBF, MockTilesWriter, make_test_file};
	use versatiles_core::utils::decompress_gzip;

	#[cfg(feature = "cli")]
	use versatiles_core::utils::PrettyPrint;

	#[tokio::test]
	async fn reader() -> Result<()> {
		let temp_file = make_test_file(TileFormat::MVT, TileCompression::Gzip, 3, "tar").await?;

		// get tar reader
		let reader = TarTilesReader::open_path(&temp_file)?;

		assert_eq!(
			format!("{reader:?}"),
			"TarTilesReader { parameters: TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1x1), 1: [0,0,1,1] (2x2), 2: [0,0,3,3] (4x4), 3: [0,0,7,7] (8x8)], tile_compression: Gzip, tile_format: MVT } }"
		);
		assert_eq!(reader.container_name(), "tar");
		assert!(reader.source_name().ends_with(temp_file.to_str().unwrap()));
		assert_eq!(
			reader.tilejson().as_string(),
			"{\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
		);
		assert_eq!(
			format!("{:?}", reader.parameters()),
			"TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1x1), 1: [0,0,1,1] (2x2), 2: [0,0,3,3] (4x4), 3: [0,0,7,7] (8x8)], tile_compression: Gzip, tile_format: MVT }"
		);
		assert_eq!(reader.parameters().tile_compression, TileCompression::Gzip);
		assert_eq!(reader.parameters().tile_format, TileFormat::MVT);

		let tile = reader.get_tile_blob(&TileCoord::new(3, 6, 2)?).await?.unwrap();
		assert_eq!(decompress_gzip(&tile)?.as_slice(), MOCK_BYTES_PBF);

		Ok(())
	}

	#[tokio::test]
	async fn all_compressions() -> Result<()> {
		async fn test_compression(compression: TileCompression) -> Result<()> {
			let temp_file = make_test_file(TileFormat::MVT, compression, 2, "tar").await?;

			// get tar reader
			let mut reader = TarTilesReader::open_path(&temp_file)?;

			MockTilesWriter::write(&mut reader).await?;
			Ok(())
		}

		test_compression(TileCompression::Uncompressed).await?;
		test_compression(TileCompression::Gzip).await?;
		test_compression(TileCompression::Brotli).await?;
		Ok(())
	}

	// Test tile fetching
	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn probe() -> Result<()> {
		let temp_file = make_test_file(TileFormat::MVT, TileCompression::Gzip, 4, "tar").await?;

		let mut reader = TarTilesReader::open_path(&temp_file)?;

		let mut printer = PrettyPrint::new();
		reader.probe_container(&printer.get_category("container").await).await?;
		assert_eq!(
			printer.as_string().await,
			"container:\n  deep container probing is not implemented for this container format\n"
		);

		let mut printer = PrettyPrint::new();
		reader.probe_tiles(&printer.get_category("tiles").await).await?;
		assert_eq!(
			printer.as_string().await,
			"tiles:\n  deep tiles probing is not implemented for this container format\n"
		);

		Ok(())
	}

	#[tokio::test]
	async fn empty_tar_file() -> Result<()> {
		let filename = assert_fs::NamedTempFile::new("empty_tar_file.tar")?;
		let file = std::fs::File::create(&filename)?;
		let mut a = tar::Builder::new(file);
		a.finish()?;

		assert_eq!(
			TarTilesReader::open_path(&filename).unwrap_err().to_string(),
			"no tiles found in tar"
		);
		Ok(())
	}

	#[tokio::test]
	async fn correct_zxy_scheme() -> Result<()> {
		let filename = assert_fs::NamedTempFile::new("correct_zxy_scheme.tar")?;
		let file = std::fs::File::create(&filename)?;
		let mut a = tar::Builder::new(file);
		let mut header = tar::Header::new_gnu();
		header.set_size(6);
		header.set_cksum();
		a.append_data(&mut header, "3/1/2.bin", [3, 1, 4, 1, 5, 9].as_ref())?;
		a.finish()?;

		let reader = TarTilesReader::open_path(&filename)?;
		assert_eq!(reader.parameters().tile_format, TileFormat::BIN);
		assert_eq!(reader.parameters().tile_compression, TileCompression::Uncompressed);
		assert_eq!(reader.parameters().bbox_pyramid.count_tiles(), 1);
		assert_eq!(
			reader
				.get_tile_blob(&TileCoord::new(3, 1, 2)?)
				.await?
				.unwrap()
				.as_slice(),
			[3, 1, 4, 1, 5, 9].as_ref()
		);
		Ok(())
	}
}

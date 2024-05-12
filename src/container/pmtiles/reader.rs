use super::types::{Directory, EntriesV3, HeaderV3, TileId};
#[cfg(feature = "full")]
use crate::helper::pretty_print::PrettyPrint;
use crate::{
	container::{TilesReaderParameters, TilesReaderTrait},
	helper::{decompress, DataReader, DataReaderFile},
	types::{Blob, ByteRange, TileBBoxPyramid, TileCompression, TileCoord3},
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use std::{fmt::Debug, path::Path};

#[derive(Debug)]
pub struct PMTilesReader {
	pub data_reader: DataReader,
	pub header: HeaderV3,
	pub meta: Blob,
	pub internal_compression: TileCompression,
	pub directory: Directory,
	pub parameters: TilesReaderParameters,
}

impl PMTilesReader {
	// Create a new TilesReader from a given filename
	pub async fn open_path(path: &Path) -> Result<PMTilesReader> {
		PMTilesReader::open_reader(DataReaderFile::from_path(path)?).await
	}

	pub async fn open_reader(mut data_reader: DataReader) -> Result<PMTilesReader>
	where
		Self: Sized,
	{
		let header = HeaderV3::deserialize(
			&data_reader
				.read_range(&ByteRange::new(0, HeaderV3::len() as u64))
				.await?,
		)?;

		let internal_compression = header.internal_compression.as_value()?;

		let meta = data_reader.read_range(&header.metadata).await?;
		let meta = decompress(meta, &internal_compression)?;

		let directory: Directory = Directory {
			root_bytes: data_reader.read_range(&header.root_dir).await?,
			leaves_bytes: data_reader.read_range(&header.leaf_dirs).await?,
		};

		let mut bbox_pyramid = TileBBoxPyramid::new_full(header.max_zoom);
		bbox_pyramid.set_zoom_min(header.min_zoom);

		let parameters = TilesReaderParameters::new(
			header.tile_type.as_value()?,
			header.tile_compression.as_value()?,
			bbox_pyramid,
		);

		Ok(PMTilesReader {
			data_reader,
			directory,
			header,
			internal_compression,
			meta,
			parameters,
		})
	}
}

#[async_trait]
impl TilesReaderTrait for PMTilesReader {
	fn get_container_name(&self) -> &str {
		"pmtiles"
	}

	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.parameters.tile_compression = tile_compression;
	}

	fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(Some(self.meta.clone()))
	}

	fn get_name(&self) -> &str {
		self.data_reader.get_name()
	}

	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		log::trace!("get_tile_data {:?}", coord);

		let tile_id: u64 = coord.get_tile_id();
		let mut dir_blob = decompress(self.directory.root_bytes.clone(), &self.internal_compression)?;

		for _depth in 0..3 {
			let entries = EntriesV3::deserialize(&dir_blob)?;
			let entry = entries.find_tile(tile_id);

			let entry = if let Some(entry) = entry {
				entry
			} else {
				return Ok(None);
			};

			if entry.range.length > 0 {
				if entry.run_length > 0 {
					return Ok(Some(
						self
							.data_reader
							.read_range(&entry.range.shift(self.header.tile_data.offset))
							.await?,
					));
				} else {
					dir_blob = self
						.data_reader
						.read_range(&entry.range.shift(self.header.leaf_dirs.offset))
						.await?;
				}
			} else {
				return Ok(None);
			}
		}

		bail!("not found")
	}

	// deep probe of container meta
	#[cfg(feature = "full")]
	async fn probe_container(&mut self, print: &PrettyPrint) -> Result<()> {
		print.add_key_value("meta size", &self.meta.len()).await;
		print.add_key_value("header", &self.header).await;

		Ok(())
	}
}

#[cfg(test)]
mod test {
	use crate::assert_wildcard;

	use super::*;
	use lazy_static::lazy_static;
	use std::{env::current_dir, path::PathBuf};

	lazy_static! {
		static ref PATH: PathBuf = current_dir().unwrap().join("./testdata/berlin.pmtiles");
	}

	#[tokio::test]
	async fn reader() -> Result<()> {
		let mut reader = PMTilesReader::open_path(&PATH).await?;

		assert_eq!(reader.get_container_name(), "pmtiles");

		assert_wildcard!(reader.get_name(), "*/testdata/berlin.pmtiles");

		assert_eq!(format!("{:?}", reader.header), "HeaderV3 { root_dir: ByteRange[127,2271], metadata: ByteRange[2398,592], leaf_dirs: ByteRange[2990,0], tile_data: ByteRange[2990,25869006], addressed_tiles_count: 878, tile_entries_count: 878, tile_contents_count: 876, clustered: true, internal_compression: Gzip, tile_compression: Gzip, tile_type: MVT, min_zoom: 0, max_zoom: 14, min_lon_e7: 130828300, min_lat_e7: 523344600, max_lon_e7: 137622450, max_lat_e7: 526783000, center_zoom: 7, center_lon_e7: 134225380, center_lat_e7: 525063800 }");

		assert_wildcard!(
			reader.get_meta()?.unwrap().as_str(),
			"{\"author\":\"OpenStreetMap contributors, Geofabrik GmbH\",*,\"version\":\"3.0\"}"
		);

		assert_wildcard!(
			format!("{:?}", reader.get_parameters()), 
			"TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1), * 14: [0,0,16383,16383] (268435456)], tile_compression: Gzip, tile_format: PBF }"
		);

		assert_eq!(
			reader.get_tile_data(&TileCoord3::new(0, 0, 0)?).await?.unwrap().len(),
			20
		);

		assert_eq!(
			reader
				.get_tile_data(&TileCoord3::new(8800, 5370, 14)?)
				.await?
				.unwrap()
				.len(),
			100391
		);

		assert!(reader.get_tile_data(&TileCoord3::new(0, 0, 16)?).await?.is_none());

		Ok(())
	}
}

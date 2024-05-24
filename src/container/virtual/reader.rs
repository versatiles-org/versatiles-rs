use super::{operations::VirtualTileOperation, output::VirtualTilesOutput};
use crate::{
	container::{
		getters::get_simple_reader, r#virtual::operations::new_virtual_tile_operation, TilesReader,
		TilesReaderParameters, TilesStream,
	},
	types::{Blob, DataReader, TileBBox, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat},
	utils::{compress, YamlWrapper},
};
use anyhow::{bail, ensure, Context, Result};
use axum::async_trait;
use futures_util::StreamExt;
use std::{collections::HashMap, path::Path, sync::Arc};
use tokio::sync::Mutex;

pub type VReader = Arc<Mutex<Box<dyn TilesReader>>>;
pub type VOperation = Arc<Box<dyn VirtualTileOperation>>;

#[derive(Clone)]
pub struct VirtualTilesReader {
	name: String,
	output_definitions: Vec<VirtualTilesOutput>,
	tiles_reader_parameters: TilesReaderParameters,
}

impl VirtualTilesReader {
	pub async fn open_path(path: &Path) -> Result<VirtualTilesReader> {
		let yaml = std::fs::read_to_string(path)?;
		Self::from_str(&yaml, path.to_str().unwrap())
			.await
			.with_context(|| format!("while parsing {path:?}"))
	}

	pub async fn open_reader(mut reader: DataReader) -> Result<VirtualTilesReader> {
		let yaml = reader.read_all().await?.into_string();
		Self::from_str(&yaml, reader.get_name())
			.await
			.with_context(|| format!("while parsing {}", reader.get_name()))
	}

	async fn from_str(yaml: &str, name: &str) -> Result<VirtualTilesReader> {
		let yaml = YamlWrapper::from_str(yaml)?;

		ensure!(yaml.is_hash(), "YAML must be an object");

		let inputs = parse_inputs(&yaml.hash_get_value("inputs")?)
			.await
			.context("while parsing 'inputs'")?;

		let operations = if yaml.hash_has_key("operations") {
			parse_operations(&yaml.hash_get_value("operations")?).context("while parsing 'operations'")?
		} else {
			HashMap::new()
		};

		let output_definitions = parse_output(&yaml.hash_get_value("output")?, &inputs, &operations)
			.await
			.context("while parsing 'output'")?;

		let tiles_reader_parameters = parse_parameters(&yaml.hash_get_value("parameters")?, &output_definitions)
			.context("while parsing 'parameters'")?;

		Ok(VirtualTilesReader {
			name: name.to_string(),
			output_definitions,
			tiles_reader_parameters,
		})
	}

	async fn get_bbox_tile_stream_small(&mut self, bbox: TileBBox) -> TilesStream {
		let n = bbox.count_tiles();

		if n > 2000 {
			panic!("two much tiles at once")
		}

		let tile_compression = self.tiles_reader_parameters.tile_compression.clone();

		for output_definition in self.output_definitions.iter_mut() {
			if !output_definition.bbox_pyramid.overlaps_bbox(&bbox) {
				continue;
			}

			let stream = output_definition.get_bbox_tile_stream(bbox).await;
			return stream
				.map(move |(coord, blob)| (coord, compress(blob, &tile_compression).unwrap()))
				.boxed();
		}

		//todo!();
		/*

		// Wrap self in an Arc<AsyncMutex<YourStruct>>
		let self_arc = Arc::new(Mutex::new(self));

		let a = stream::iter(bboxes).map(|bbox| {
			let self_arc_clone = self_arc.clone();
			async move {
				let mut locked_self = self_arc_clone.lock().await;
				locked_self.get_bbox_tile_stream_small(bbox)
			}
		});
		*/

		//todo!();
		// Create a FuturesUnordered

		// Collect the resulting streams
		//let streams: Vec<TilesStream> = futures.collect();

		// Combine all streams into one using select_all
		//let combined_stream = select_all(streams);

		// Box and pin the combined stream
		//Box::pin(combined_stream)

		// limit bbox to 1024x1024
		// note already deliviered tiles in a [[bool]]
		// get bbox of tile not fetched yet
		// ask next output

		/*
		for output_definition in self.output_definitions.iter() {
			if !output_definition.bbox_pyramid.contains_coord(bbox) {
				continue;
			}
			if let Some(mut tile) = output_definition.get_tile_data(coord).await? {
				tile = compress(tile, &self.tiles_reader_parameters.tile_compression)?;
				return Ok(Some(tile));
			} else {
				continue;
			}
		}

		let mutex = Arc::new(Mutex::new(self));
		let coords: Vec<TileCoord3> = bbox.iter_coords().collect();
		stream::iter(coords)
			.filter_map(move |coord| {
				let mutex = mutex.clone();
				async move {
					mutex
						.lock()
						.await
						.get_tile_data(&coord)
						.await
						.map(|blob_option| blob_option.map(|blob| (coord, blob)))
						.unwrap_or(None)
				}
			})
			.boxed()
			 */
		todo!()
	}
}

async fn parse_inputs(yaml: &YamlWrapper) -> Result<HashMap<String, VReader>> {
	ensure!(yaml.is_hash(), "'inputs' must be an object");

	let mut inputs: HashMap<String, VReader> = HashMap::new();

	for (name, entry) in yaml.hash_get_as_vec()? {
		let filename = entry.hash_get_str("filename")?;
		if inputs.contains_key(&name) {
			bail!("input '{name}' is duplicated")
		}
		inputs.insert(name, Arc::new(Mutex::new(get_simple_reader(filename).await?)));
	}

	if inputs.is_empty() {
		bail!("YAML needs at least one input")
	}

	Ok(inputs)
}

fn parse_operations(yaml: &YamlWrapper) -> Result<HashMap<String, VOperation>> {
	ensure!(yaml.is_hash(), "'operations' must be an object");

	let mut operations: HashMap<String, VOperation> = HashMap::new();

	for (index, (name, entry)) in yaml.hash_get_as_vec()?.iter().enumerate() {
		operations.insert(
			name.to_string(),
			Arc::new(
				new_virtual_tile_operation(entry).with_context(|| format!("while parsing operation no {}", index + 1))?,
			),
		);
	}

	Ok(operations)
}

async fn parse_output(
	yaml: &YamlWrapper, input_lookup: &HashMap<String, VReader>, operation_lookup: &HashMap<String, VOperation>,
) -> Result<Vec<VirtualTilesOutput>> {
	ensure!(yaml.is_array(), "'output' must be an array");

	let mut output: Vec<VirtualTilesOutput> = Vec::new();

	for (index, entry) in yaml.array_get_as_vec()?.iter().enumerate() {
		output.push(
			VirtualTilesOutput::new(entry, input_lookup, operation_lookup)
				.await
				.with_context(|| format!("while parsing output no {}", index + 1))?,
		);
	}

	Ok(output)
}

fn parse_parameters(yaml: &YamlWrapper, outputs: &Vec<VirtualTilesOutput>) -> Result<TilesReaderParameters> {
	ensure!(yaml.is_hash(), "'parameters' must be an object");
	let tile_compression = TileCompression::from_str(yaml.hash_get_str("compression")?)?;
	let tile_format = TileFormat::from_str(yaml.hash_get_str("format")?)?;

	let mut bbox_pyramid = TileBBoxPyramid::new_empty();
	for output in outputs.iter() {
		bbox_pyramid.include_bbox_pyramid(&output.bbox_pyramid);
	}

	Ok(TilesReaderParameters {
		bbox_pyramid,
		tile_compression,
		tile_format,
	})
}

#[async_trait]
impl TilesReader for VirtualTilesReader {
	/// Get the name of the reader source, e.g., the filename.
	fn get_name(&self) -> &str {
		&self.name
	}

	/// Get the container name, e.g., versatiles, mbtiles, etc.
	fn get_container_name(&self) -> &str {
		"virtual"
	}

	/// Get the reader parameters.
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.tiles_reader_parameters
	}

	/// Override the tile compression.
	fn override_compression(&mut self, _tile_compression: TileCompression) {
		panic!("you can't override the compression of virtual tile sources")
	}

	/// Get the metadata, always uncompressed.
	fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(None)
	}

	/// Get tile data for the given coordinate, always compressed and formatted.
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		for output_definition in self.output_definitions.iter() {
			if !output_definition.bbox_pyramid.contains_coord(coord) {
				continue;
			}
			if let Some(mut tile) = output_definition.get_tile_data(coord).await? {
				tile = compress(tile, &self.tiles_reader_parameters.tile_compression)?;
				return Ok(Some(tile));
			} else {
				continue;
			}
		}
		Ok(None)
	}

	/// Get a stream of tiles within the bounding box.
	async fn get_bbox_tile_stream(&mut self, bbox: TileBBox) -> TilesStream {
		let bboxes: Vec<TileBBox> = bbox.iter_bbox_grid(32).collect();

		let self_mutex = Arc::new(Mutex::new(self));

		futures_util::stream::iter(bboxes)
			.then(move |bbox| {
				let self_mutex = self_mutex.clone();
				async move {
					let mut myself = self_mutex.lock().await;
					let stream = myself.get_bbox_tile_stream_small(bbox).await;
					let entries: Vec<(TileCoord3, Blob)> = stream.collect().await;
					futures_util::stream::iter(entries)
				}
			})
			.flatten()
			.boxed()
	}
}

impl std::fmt::Debug for VirtualTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("VirtualTilesReader")
			.field("name", &self.name)
			.field("reader parameters", &self.tiles_reader_parameters)
			.field("output definitions", &self.output_definitions)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use crate::container::MockTilesWriter;

	use super::*;

	#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
	async fn open_yaml() -> Result<()> {
		let mut reader = VirtualTilesReader::open_path(&Path::new("testdata/test.yaml")).await?;
		MockTilesWriter::write(&mut reader).await?;

		Ok(())
	}
}

#![deny(clippy::all)]

mod container;
mod server;
mod types;
mod utils;

pub use container::ContainerReader;
pub use server::TileServer;
pub use types::{ConvertOptions, ProbeResult, ReaderParameters, ServerOptions, TileCoord};

use napi::bindgen_prelude::*;
use napi_derive::napi;
use versatiles_container::{
	ContainerRegistry, TilesConverterParameters, convert_tiles_container,
};
use versatiles_core::{GeoBBox, TileBBoxPyramid};

/// Convert tiles from one format to another
///
/// Supports converting between .versatiles, .mbtiles, .pmtiles, .tar, and directories
///
/// # Example
/// ```javascript
/// await convertTiles(
///   'input.mbtiles',
///   'output.versatiles',
///   {
///     minZoom: 0,
///     maxZoom: 14,
///     bbox: [-180, -85, 180, 85],
///     compress: 'gzip'
///   }
/// );
/// ```
#[napi]
pub async fn convert_tiles(
	input: String,
	output: String,
	options: Option<ConvertOptions>,
) -> Result<()> {
	let registry = ContainerRegistry::default();
	let reader = napi_result!(registry.get_reader_from_str(&input).await)?;

	let opts = options.unwrap_or(ConvertOptions {
		min_zoom: None,
		max_zoom: None,
		bbox: None,
		bbox_border: None,
		compress: None,
		flip_y: None,
		swap_xy: None,
	});

	let mut bbox_pyramid: Option<TileBBoxPyramid> = None;

	if opts.min_zoom.is_some() || opts.max_zoom.is_some() || opts.bbox.is_some() {
		let mut pyramid = TileBBoxPyramid::new_full(32);

		if let Some(min) = opts.min_zoom {
			pyramid.set_level_min(min);
		}

		if let Some(max) = opts.max_zoom {
			pyramid.set_level_max(max);
		}

		if let Some(bbox_vec) = opts.bbox {
			if bbox_vec.len() != 4 {
				return Err(Error::from_reason(
					"bbox must contain exactly 4 numbers [west, south, east, north]",
				));
			}
			let geo_bbox = napi_result!(GeoBBox::try_from(bbox_vec))?;
			napi_result!(pyramid.intersect_geo_bbox(&geo_bbox))?;

			if let Some(border) = opts.bbox_border {
				pyramid.add_border(border, border, border, border);
			}
		}

		bbox_pyramid = Some(pyramid);
	}

	let tile_compression = if let Some(ref comp_str) = opts.compress {
		Some(types::parse_compression(comp_str).ok_or_else(|| {
			Error::from_reason(format!(
				"Invalid compression '{}'. Use 'gzip', 'brotli', or 'uncompressed'",
				comp_str
			))
		})?)
	} else {
		None
	};

	let params = TilesConverterParameters {
		bbox_pyramid,
		tile_compression,
		flip_y: opts.flip_y.unwrap_or(false),
		swap_xy: opts.swap_xy.unwrap_or(false),
	};

	let output_path = std::path::PathBuf::from(&output);

	napi_result!(convert_tiles_container(reader, params, &output_path, registry).await)?;

	Ok(())
}

/// Probe a tile container to get metadata and statistics
///
/// # Example
/// ```javascript
/// const result = await probeTiles('tiles.mbtiles', 'shallow');
/// console.log(result.tileJson);
/// console.log(result.parameters);
/// ```
#[napi]
pub async fn probe_tiles(path: String, _depth: Option<String>) -> Result<ProbeResult> {
	let registry = ContainerRegistry::default();
	let reader = napi_result!(registry.get_reader_from_str(&path).await)?;

	Ok(ProbeResult {
		source_name: reader.source_name().to_string(),
		container_name: reader.container_name().to_string(),
		tile_json: reader.tilejson().as_string(),
		parameters: ReaderParameters::from(reader.parameters()),
	})
}

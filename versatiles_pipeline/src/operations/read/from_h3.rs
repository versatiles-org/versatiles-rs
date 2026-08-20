//! # H3 hexagon grid source
//!
//! Generates vector tiles filled with [H3](https://h3geo.org) cells — one
//! polygon per hexagon (or pentagon), carrying its H3 index as a string
//! property. Nothing is read from disk: the cells covering a tile are computed
//! when that tile is asked for, so no part of the grid is ever held in memory.
//!
//! The point of a generated grid is joining data to it. Gridded statistics are
//! published as tables keyed on a cell id — `h3` in the Kontur population
//! dataset, for example — so the polygons only become useful once
//! `vector_update_properties` matches that column against the id property here:
//!
//! ```text
//! from_h3 resolution=8 bbox=[13.0,52.3,13.8,52.7]
//!   | vector_update_properties data_source_path="population.csv"
//!       id_field_tiles="h3" id_field_data="h3"
//! ```
//!
//!
//! ## Cells and tiles
//!
//! A cell reaching into several tiles is drawn in each of them, clipped to the
//! tile it is in. The same id therefore recurs across tiles, which is what a
//! join expects — but the geometry carrying it in any one tile is only the part
//! that falls inside that tile, so it is not something to measure areas from.
//! ## Zoom levels
//!
//! Cell size is fixed by the resolution and does not change with zoom — that is
//! what keeps an id stable enough to join against. Low zoom levels are
//! therefore unusable, since one tile would hold every cell in view, so the
//! source derives its own minimum zoom from the resolution and only advertises
//! levels at or above it. See [`min_zoom_for_cell_size`].

use std::{fmt::Debug, sync::Arc};

use anyhow::{Result, anyhow, ensure};
use async_trait::async_trait;
use geo_types::{Coord, Geometry, LineString, Polygon, coord};
use h3o::{
	CellIndex, Resolution,
	geom::{ContainmentMode, TilerBuilder},
};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{GeoBBox, TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream};
use versatiles_derive::context;
use versatiles_geometry::{
	ext::mercator::coord_to_mercator,
	feature_import::{TILE_EXTENT, render_tile},
	geo::{GeoFeature, GeoProperties, GeoValue},
};

use crate::{
	PipelineFactory,
	helpers::{grid_zoom::min_zoom_for_cell_size, tilejson::set_single_vector_layer},
	vpl::VPLNode,
};

/// Cells per tile the derived minimum zoom aims for when nothing is given.
const DEFAULT_MAX_CELLS_PER_TILE: u32 = 1024;

/// Zoom levels published above the derived minimum when nothing is given.
///
/// A grid is mostly looked at around the zoom where its cells are legible, and
/// every extra level multiplies the tile count by four while showing the same
/// hexagons, so the default is deliberately shallow.
const DEFAULT_ZOOM_RANGE: u8 = 3;

/// Largest zoom level a tile pyramid may use.
const MAX_ZOOM: u8 = 30;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Generates vector tiles containing H3 grid cells, ready to be joined with data keyed on the H3 index.
struct Args {
	/// H3 resolution, `0` (cells of ~4,250,000 km²) to `15` (~0.9 m²).
	/// For example: `resolution=8` for cells of roughly 0.7 km².
	/// See <https://h3geo.org/docs/core-library/restable/> for the full table.
	resolution: u8,

	/// The area to cover, as `[west, south, east, north]` in WGS84 degrees.
	/// For example: `bbox=[13.0,52.3,13.8,52.7]` for Berlin.
	/// Required: a grid without bounds would be generated for the whole planet,
	/// which at most resolutions is more tiles than can be written.
	bbox: [f64; 4],

	/// Roughly how many cells one tile may hold. Decides the lowest zoom level
	/// this source offers: below it a single tile would carry more cells than a
	/// tile can usefully hold. Default: `1024`.
	max_cells_per_tile: Option<u32>,

	/// Highest zoom level to generate. Defaults to three levels above the
	/// derived minimum, since further levels repeat the same cells.
	max_zoom: Option<u8>,

	/// Name of the layer in the generated tiles. Default: `"grid"`.
	layer_name: Option<String>,

	/// Name of the string property holding the H3 index, e.g. `"8928308280fffff"`.
	/// Default: `"h3"`, matching how H3 datasets usually name the column.
	id_field: Option<String>,
}

/// A [`TileSource`] that fabricates H3 cells per requested tile.
#[derive(Debug)]
pub struct Operation {
	resolution: Resolution,
	layer_name: String,
	id_field: String,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
}

impl Operation {
	#[context("Failed to build from_h3 operation")]
	fn from_args(args: Args) -> Result<Self> {
		let resolution = Resolution::try_from(args.resolution)
			.map_err(|_| anyhow!("resolution must be between 0 and 15, got {}", args.resolution))?;

		let bbox = GeoBBox::new(args.bbox[0], args.bbox[1], args.bbox[2], args.bbox[3])?;
		let max_cells_per_tile = args.max_cells_per_tile.unwrap_or(DEFAULT_MAX_CELLS_PER_TILE);
		ensure!(max_cells_per_tile > 0, "max_cells_per_tile must be at least 1");

		let min_zoom = min_zoom_for_cell_size(cell_size_mercator(resolution, &bbox), max_cells_per_tile);
		let max_zoom = args
			.max_zoom
			.unwrap_or_else(|| min_zoom.saturating_add(DEFAULT_ZOOM_RANGE).min(MAX_ZOOM));
		ensure!(
			max_zoom >= min_zoom,
			"max_zoom ({max_zoom}) is below the zoom this grid starts at ({min_zoom}): resolution {} needs zoom {min_zoom} \
			 before a tile holds at most {max_cells_per_tile} cells. Raise max_zoom, raise max_cells_per_tile, or pick a \
			 coarser resolution.",
			args.resolution
		);

		let layer_name = args.layer_name.unwrap_or_else(|| String::from("grid"));
		let id_field = args.id_field.unwrap_or_else(|| String::from("h3"));

		log::info!(
			"from_h3: resolution {} covers zoom {min_zoom}..={max_zoom} (at most ~{max_cells_per_tile} cells per tile)",
			args.resolution
		);

		let pyramid = TilePyramid::from_geo_bbox(min_zoom, max_zoom, &bbox)?;
		let metadata = TileSourceMetadata::new(
			TileFormat::MVT,
			TileCompression::Uncompressed,
			Traversal::ANY,
			Some(pyramid),
		);

		let mut tilejson = TileJSON::default();
		tilejson.set_string("name", &layer_name)?;
		set_single_vector_layer(
			&mut tilejson,
			&layer_name,
			std::iter::once((id_field.clone(), String::from("String"))).collect(),
			min_zoom,
			max_zoom,
		);
		metadata.update_tilejson(&mut tilejson);

		Ok(Self {
			resolution,
			layer_name,
			id_field,
			metadata,
			tilejson,
		})
	}
}

impl Operation {
	#[context("Failed to build from_h3 operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, _factory: &PipelineFactory) -> Result<Box<dyn TileSource>> {
		Ok(Box::new(Operation::from_args(Args::from_vpl_node(&vpl_node)?)?) as Box<dyn TileSource>)
	}
}

#[async_trait]
impl TileSource for Operation {
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("h3 grid", "h3")
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		let resolution = self.resolution;
		let layer_name = self.layer_name.clone();
		let id_field = self.id_field.clone();

		Ok(TileStream::from_bbox_parallel(bbox, move |coord| {
			// The inputs are the tile coordinate and a resolution, both already
			// validated, so a failure here is a bug rather than bad data.
			build_tile(coord, resolution, &layer_name, &id_field).expect("H3 cells form valid geometry")
		}))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |_coord| {
			Some(())
		}))
	}
}

/// The size of one cell in mercator meters, where mercator makes it smallest.
///
/// H3 areas are measured on the sphere; mercator stretches them by `1/cos(lat)`,
/// so cells are smallest — and therefore densest per tile — at the latitude in
/// `bbox` closest to the equator. Measuring there keeps the derived minimum zoom
/// on the safe side.
fn cell_size_mercator(resolution: Resolution, bbox: &GeoBBox) -> f64 {
	let lat = if bbox.y_min <= 0.0 && bbox.y_max >= 0.0 {
		0.0
	} else {
		bbox.y_min.abs().min(bbox.y_max.abs())
	};
	// Compare cells as squares of equal area: close enough for choosing a zoom,
	// and it avoids pretending a hexagon has one width.
	resolution.area_m2().sqrt() / lat.to_radians().cos()
}

/// Build one tile: every cell touching it, clipped and encoded.
fn build_tile(coord: TileCoord, resolution: Resolution, layer_name: &str, id_field: &str) -> Result<Option<Tile>> {
	let features = cells_covering(coord, resolution)?
		.into_iter()
		.map(|cell| cell_feature(cell, id_field))
		.collect::<Vec<_>>();

	// `render_tile` clips to the tile, quantizes to the MVT grid and drops the
	// tile if nothing usable survives — so a cell crossing a tile border is cut
	// at the border and appears in both tiles.
	let tile = render_tile(features, layer_name, coord.to_mercator_bbox(), TILE_EXTENT)?
		.map(|vector_tile| Tile::from_vector(vector_tile, TileFormat::MVT))
		.transpose()?;

	Ok(tile)
}

/// Every cell whose area meets the tile, including those only partly inside.
#[context("Failed to compute H3 coverage for {coord:?}")]
fn cells_covering(coord: TileCoord, resolution: Resolution) -> Result<Vec<CellIndex>> {
	let bbox = coord.to_geo_bbox();
	let ring = LineString::from(vec![
		coord! { x: bbox.x_min, y: bbox.y_min },
		coord! { x: bbox.x_max, y: bbox.y_min },
		coord! { x: bbox.x_max, y: bbox.y_max },
		coord! { x: bbox.x_min, y: bbox.y_max },
		coord! { x: bbox.x_min, y: bbox.y_min },
	]);

	let mut builder = TilerBuilder::new(resolution)
		// `Covers` rather than `ContainsCentroid`: a cell that merely overlaps
		// the tile still has to be drawn in it, and at high zoom the tile may sit
		// entirely inside a single cell, which only this mode reports.
		.containment_mode(ContainmentMode::Covers);
	if bbox.x_max - bbox.x_min >= 180.0 {
		// The heuristic reads a wide tile as one that wraps the antimeridian.
		// Tiles never do — they are cut from a rectangle in mercator — so at low
		// zoom, where a tile is wider than half the world, it has to be off.
		builder = builder.disable_transmeridian_heuristic();
	}

	let mut tiler = builder.build();
	tiler.add(Polygon::new(ring, Vec::new()))?;
	Ok(tiler.into_coverage().collect())
}

/// One cell as a mercator polygon carrying its index as a string property.
fn cell_feature(cell: CellIndex, id_field: &str) -> GeoFeature {
	let boundary = cell.boundary();
	let mut coords: Vec<Coord<f64>> = Vec::with_capacity(boundary.len() + 1);

	// A cell straddling the antimeridian reports longitudes on both sides of it,
	// which would project into a polygon spanning the whole world. Keeping every
	// vertex on the same side as the first one puts the cell just past ±180
	// instead, where the tile clip cuts it correctly.
	let first_lng = boundary.first().map_or(0.0, |ll| ll.lng());
	for ll in boundary.iter() {
		let mut lng = ll.lng();
		if lng - first_lng > 180.0 {
			lng -= 360.0;
		} else if first_lng - lng > 180.0 {
			lng += 360.0;
		}
		coords.push(coord_to_mercator(coord! { x: lng, y: ll.lat() }));
	}
	if let Some(first) = coords.first().copied() {
		coords.push(first);
	}

	let mut properties = GeoProperties::new();
	properties.insert(id_field.to_string(), GeoValue::from(cell.to_string()));

	GeoFeature {
		// MVT feature ids are `u64` and an H3 index is published as a hex string,
		// so the id lives in a property; setting a numeric id as well would offer
		// a second, different key for the same cell.
		id: None,
		geometry: Geometry::Polygon(Polygon::new(LineString::from(coords), Vec::new())),
		properties,
	}
}

crate::operations::macros::define_read_factory!("from_h3", Args, Operation);

#[cfg(test)]
mod tests {
	use h3o::LatLng;
	use versatiles_core::{TileCompression::Uncompressed, WORLD_SIZE};
	use versatiles_geometry::vector_tile::VectorTile;

	use super::*;
	use crate::PipelineFactory;

	/// Berlin, well away from the poles and the antimeridian.
	const LAT: f64 = 52.52;
	const LNG: f64 = 13.405;
	const BBOX: &str = "bbox=[13.0,52.3,13.8,52.7]";

	async fn build(vpl: &str) -> Result<Box<dyn TileSource>> {
		PipelineFactory::new_dummy().operation_from_vpl(vpl).await
	}

	fn ids_in_tile(vt: &VectorTile) -> Vec<String> {
		vt.layers[0]
			.to_features()
			.unwrap()
			.iter()
			.map(|f| match f.properties.get("h3") {
				Some(GeoValue::String(s)) => s.clone(),
				other => panic!("expected a string id, got {other:?}"),
			})
			.collect()
	}

	#[tokio::test]
	async fn resolution_decides_the_zoom_range() -> Result<()> {
		// Resolution 8 cells are ~0.74 km², so ~0.86 km across on the ground and
		// ~1.4 km in mercator at this latitude: 1024 of them fit in a tile from
		// zoom 10 down.
		let op = build(&format!("from_h3 resolution=8 {BBOX}")).await?;
		let pyramid = op.tile_pyramid().await?;
		assert_eq!(pyramid.level_min(), Some(10));
		assert_eq!(pyramid.level_max(), Some(13));

		// Coarser cells are servable earlier.
		let op = build(&format!("from_h3 resolution=4 {BBOX}")).await?;
		let pyramid = op.tile_pyramid().await?;
		assert_eq!(pyramid.level_min(), Some(5));
		Ok(())
	}

	#[tokio::test]
	async fn a_bigger_budget_lowers_the_minimum_zoom() -> Result<()> {
		let op = build(&format!("from_h3 resolution=8 {BBOX} max_cells_per_tile=16384")).await?;
		assert_eq!(op.tile_pyramid().await?.level_min(), Some(8));
		Ok(())
	}

	#[tokio::test]
	async fn the_tile_holding_a_point_carries_that_point_s_cell() -> Result<()> {
		let op = build(&format!("from_h3 resolution=8 {BBOX}")).await?;
		let cell = LatLng::new(LAT, LNG)?.to_cell(Resolution::Eight);
		let coord = TileCoord::from_geo(LNG, LAT, 10)?;

		let tile = op.tile(&coord).await?.expect("tile present");
		let vt = VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?;

		assert_eq!(vt.layers[0].name, "grid");
		assert!(
			ids_in_tile(&vt).contains(&cell.to_string()),
			"the cell containing the point should be in the tile containing the point"
		);
		Ok(())
	}

	#[tokio::test]
	async fn a_cell_crossing_a_tile_border_is_in_both_tiles() -> Result<()> {
		let coord = TileCoord::from_geo(LNG, LAT, 10)?;
		let right = TileCoord::new(coord.level, coord.x + 1, coord.y)?;

		let here = cells_covering(coord, Resolution::Eight)?;
		let there = cells_covering(right, Resolution::Eight)?;

		let shared = here.iter().filter(|cell| there.contains(cell)).count();
		assert!(
			shared > 0,
			"cells straddling the border must be generated for both tiles, else the grid gaps at every tile edge"
		);
		Ok(())
	}

	#[tokio::test]
	async fn a_tile_inside_a_single_cell_still_gets_that_cell() -> Result<()> {
		// At zoom 20 a tile is a few dozen meters across, far smaller than a
		// resolution 8 cell, so nothing intersects its boundary — only
		// `ContainmentMode::Covers` reports the cell that swallows it.
		let coord = TileCoord::from_geo(LNG, LAT, 20)?;
		let cells = cells_covering(coord, Resolution::Eight)?;
		assert_eq!(cells.len(), 1);
		assert_eq!(cells[0], LatLng::new(LAT, LNG)?.to_cell(Resolution::Eight));
		Ok(())
	}

	#[test]
	fn a_cell_at_the_antimeridian_stays_in_one_piece() {
		// Its vertices sit on both sides of ±180°; taken at face value they
		// would project into a polygon spanning the whole world.
		let cell = LatLng::new(0.0, 179.999).unwrap().to_cell(Resolution::Eight);
		let feature = cell_feature(cell, "h3");
		let Geometry::Polygon(polygon) = feature.geometry else {
			panic!("expected a polygon");
		};

		let xs: Vec<f64> = polygon.exterior().coords().map(|c| c.x).collect();
		let span = xs.iter().copied().fold(f64::MIN, f64::max) - xs.iter().copied().fold(f64::MAX, f64::min);
		assert!(span < WORLD_SIZE / 100.0, "cell spans {span} meters");
	}

	#[test]
	fn cells_are_smallest_in_mercator_near_the_equator() {
		let at =
			|y_min: f64, y_max: f64| cell_size_mercator(Resolution::Eight, &GeoBBox::new(0.0, y_min, 1.0, y_max).unwrap());
		assert!(at(0.0, 1.0) < at(50.0, 51.0));
		assert!(at(50.0, 51.0) < at(70.0, 71.0));
		// A bbox spanning the equator is measured there, where mercator stretches
		// least — the same as a bbox sitting on it.
		assert!((at(-10.0, 10.0) - at(0.0, 1.0)).abs() < f64::EPSILON);
	}

	#[tokio::test]
	async fn the_id_property_can_be_renamed() -> Result<()> {
		let op = build(&format!(
			"from_h3 resolution=8 {BBOX} id_field=\"cell\" layer_name=\"hexes\""
		))
		.await?;
		let tile = op
			.tile(&TileCoord::from_geo(LNG, LAT, 10)?)
			.await?
			.expect("tile present");
		let vt = VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?;

		assert_eq!(vt.layers[0].name, "hexes");
		let features = vt.layers[0].to_features()?;
		assert!(features[0].properties.get("cell").is_some());
		assert!(features[0].properties.get("h3").is_none());
		Ok(())
	}

	#[tokio::test]
	async fn features_carry_no_numeric_id() -> Result<()> {
		// An H3 index does not fit a u64 MVT id as published, so the string
		// property is the only key; a numeric id would be a second, different one.
		let op = build(&format!("from_h3 resolution=8 {BBOX}")).await?;
		let tile = op
			.tile(&TileCoord::from_geo(LNG, LAT, 10)?)
			.await?
			.expect("tile present");
		let vt = VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?;
		assert!(vt.layers[0].features.iter().all(|f| f.id.is_none()));
		Ok(())
	}

	/// The reason this source exists: cells joined to a table keyed on the index.
	#[tokio::test]
	async fn cells_can_be_joined_with_data_keyed_on_the_index() -> Result<()> {
		let cell = LatLng::new(LAT, LNG)?.to_cell(Resolution::Eight);
		let dir = assert_fs::TempDir::new()?;
		let csv = dir.path().join("population.csv");
		std::fs::write(&csv, format!("h3,population\n{cell},4321\n"))?;

		let op = build(&format!(
			"from_h3 resolution=8 {BBOX} | vector_update_properties data_source_path=\"{}\" \
			 layer_name=\"grid\" id_field_tiles=\"h3\" id_field_data=\"h3\"",
			// A Windows path is full of backslashes, and VPL reads those as escapes.
			csv.to_string_lossy().replace('\\', "\\\\")
		))
		.await?;

		let tile = op
			.tile(&TileCoord::from_geo(LNG, LAT, 10)?)
			.await?
			.expect("tile present");
		let vt = VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?;
		let features = vt.layers[0].to_features()?;

		let joined = features
			.iter()
			.find(|f| f.properties.get("h3") == Some(&GeoValue::from(cell.to_string())))
			.expect("the cell is in this tile");
		assert_eq!(joined.properties.get("population"), Some(&GeoValue::from(4321)));

		// Every other cell keeps its id and gains nothing.
		let joined_count = features
			.iter()
			.filter(|f| f.properties.get("population").is_some())
			.count();
		assert_eq!(joined_count, 1);
		Ok(())
	}

	#[tokio::test]
	async fn tilejson_declares_the_layer_and_its_field() -> Result<()> {
		let op = build(&format!("from_h3 resolution=8 {BBOX}")).await?;
		let layers = &op.tilejson().vector_layers.0;
		let layer = layers.get("grid").expect("layer declared");
		assert_eq!(layer.fields.get("h3").map(String::as_str), Some("String"));
		assert_eq!(layer.minzoom, Some(10));
		assert_eq!(layer.maxzoom, Some(13));
		Ok(())
	}

	#[tokio::test]
	async fn bad_arguments_are_rejected() {
		let err = format!(
			"{:?}",
			build(&format!("from_h3 resolution=16 {BBOX}")).await.unwrap_err()
		);
		assert!(err.contains("between 0 and 15"), "{err}");

		let err = format!(
			"{:?}",
			build(&format!("from_h3 resolution=8 {BBOX} max_zoom=3"))
				.await
				.unwrap_err()
		);
		assert!(err.contains("below the zoom this grid starts at"), "{err}");

		let err = format!("{:?}", build("from_h3 resolution=8").await.unwrap_err());
		assert!(err.contains("bbox"), "{err}");
	}
}

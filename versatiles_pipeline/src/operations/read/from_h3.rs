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
//! from_h3 resolution=8
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
//! ## Extent and zoom levels
//!
//! The grid is the whole planet at every zoom level it can be served at: cells
//! are computed per requested tile, so an unbounded source costs nothing until
//! something asks for a tile. Restricting the area is the consumer's business —
//! `versatiles convert --bbox --max-zoom`, or a `filter` operation in the
//! pipeline — which is why there is no `bbox` or `max_zoom` here to disagree
//! with it.
//!
//! Cell size is fixed by the resolution and does not change with zoom — that is
//! what keeps an id stable enough to join against. Low zoom levels are
//! therefore unusable, since one tile would hold every cell in view, so the
//! source derives its own minimum zoom from the resolution and advertises every
//! level from there to [`MAX_ZOOM_LEVEL`]. See [`min_zoom_for_cell_size`].

use std::{fmt::Debug, sync::Arc};

use anyhow::{Result, anyhow, ensure};
use async_trait::async_trait;
use geo_types::{Coord, Geometry, LineString, Polygon, coord};
use h3o::{
	CellIndex, Resolution,
	geom::{ContainmentMode, TilerBuilder},
};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{
	MAX_ZOOM_LEVEL, TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream,
};
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

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Generates vector tiles holding H3 hexagons.
///
/// The hexagonal counterpart to `from_grid`: data published as a table keyed on
/// an H3 index gets the geometry that index refers to, ready to be joined onto
/// with `vector_update_properties`.
///
/// Resolution `0` gives cells of about 4,250,000 km² and `15` about 0.9 m²;
/// `resolution=8` lands near 0.7 km². The full table of cell areas and edge
/// lengths is at <https://h3geo.org/docs/core-library/restable/>.
///
/// The source covers the whole planet, from the zoom its cells become legible
/// at up to level 30. Nothing is generated until a tile is asked for, so bound
/// the work where it is done instead: `versatiles convert --bbox --max-zoom`, or
/// a `filter` operation.
struct Args {
	/// H3 resolution, `0` (coarsest) to `15` (finest).
	resolution: u8,

	/// Roughly how many cells one tile may hold. Defaults to `1024`.
	#[vpl(default = "1024")]
	max_cells_per_tile: Option<u32>,

	/// Name of the layer to write into. Defaults to `grid`.
	#[vpl(default = "grid")]
	layer_name: Option<String>,

	/// Property holding the H3 index. Defaults to `h3`.
	#[vpl(default = "h3")]
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

		let max_cells_per_tile = args.max_cells_per_tile.unwrap_or(DEFAULT_MAX_CELLS_PER_TILE);
		ensure!(max_cells_per_tile > 0, "max_cells_per_tile must be at least 1");

		let min_zoom = min_zoom_for_cell_size(cell_size_mercator(resolution), max_cells_per_tile);
		let max_zoom = MAX_ZOOM_LEVEL;

		let layer_name = args.layer_name.unwrap_or_else(|| String::from("grid"));
		let id_field = args.id_field.unwrap_or_else(|| String::from("h3"));

		log::info!(
			"from_h3: resolution {} covers zoom {min_zoom}..={max_zoom} worldwide (at most ~{max_cells_per_tile} cells \
			 per tile); restrict a conversion with --bbox / --max-zoom",
			args.resolution
		);

		// The whole world from `min_zoom` up. Levels below it are cleared rather
		// than served badly: there a single tile would hold every cell in view.
		let mut pyramid = TilePyramid::new_full_up_to(max_zoom);
		pyramid.set_level_min(min_zoom);
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
/// A cell's edge length in mercator meters, measured at the equator.
///
/// Mercator stretches with latitude, so a cell of fixed ground area covers more
/// mercator meters the further it sits from the equator. Measuring at the
/// equator is the conservative end: it yields the smallest mercator size, hence
/// the highest minimum zoom, so no part of the world is served at a level where
/// its tiles would be overfull.
fn cell_size_mercator(resolution: Resolution) -> f64 {
	// Compare cells as squares of equal area: close enough for choosing a zoom,
	// and it avoids pretending a hexagon has one width.
	resolution.area_m2().sqrt()
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

	/// The resolution decides where the pyramid starts; it always runs to the end.
	///
	/// Cells are computed per requested tile, so advertising every level above
	/// the minimum costs nothing until something asks for one. Bounding the work
	/// belongs to whoever does it — `convert --bbox --max-zoom`, or `filter`.
	#[tokio::test]
	async fn resolution_decides_where_the_pyramid_starts() -> Result<()> {
		// Resolution 8 cells are ~0.74 km², so ~0.86 km across on the ground and
		// in mercator at the equator, where they are smallest: 1024 of them fit
		// in a tile from zoom 11 down.
		let pyramid = build("from_h3 resolution=8").await?.tile_pyramid().await?;
		assert_eq!(pyramid.level_min(), Some(11));
		assert_eq!(pyramid.level_max(), Some(MAX_ZOOM_LEVEL));

		// Coarser cells are servable earlier, and still to the end.
		let pyramid = build("from_h3 resolution=4").await?.tile_pyramid().await?;
		assert_eq!(pyramid.level_min(), Some(5));
		assert_eq!(pyramid.level_max(), Some(MAX_ZOOM_LEVEL));
		Ok(())
	}

	/// The grid covers the planet, not the area someone happened to ask about.
	#[tokio::test]
	async fn the_pyramid_spans_the_world() -> Result<()> {
		let pyramid = build("from_h3 resolution=4").await?.tile_pyramid().await?;
		let bbox = pyramid.geo_bbox().expect("bounds");
		assert!(bbox.x_min <= -179.9 && bbox.x_max >= 179.9, "{bbox:?}");
		assert!(bbox.y_min <= -85.0 && bbox.y_max >= 85.0, "{bbox:?}");
		Ok(())
	}

	#[tokio::test]
	async fn a_bigger_budget_lowers_the_minimum_zoom() -> Result<()> {
		let op = build("from_h3 resolution=8 max_cells_per_tile=16384").await?;
		assert_eq!(op.tile_pyramid().await?.level_min(), Some(9));
		Ok(())
	}

	#[tokio::test]
	async fn the_tile_holding_a_point_carries_that_point_s_cell() -> Result<()> {
		let op = build("from_h3 resolution=8").await?;
		let cell = LatLng::new(LAT, LNG)?.to_cell(Resolution::Eight);
		let coord = TileCoord::from_geo(LNG, LAT, 11)?;

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
		let coord = TileCoord::from_geo(LNG, LAT, 11)?;
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

	/// Cell size is measured at the equator, where mercator stretches least.
	///
	/// That is the conservative end: it gives the smallest mercator size and so
	/// the highest minimum zoom, which is the one that holds everywhere. A grid
	/// serving the whole world has no single latitude to measure at, so it takes
	/// the one that cannot leave a tile overfull.
	#[test]
	fn cells_are_measured_at_the_equator() {
		let size = cell_size_mercator(Resolution::Eight);
		assert!((size - Resolution::Eight.area_m2().sqrt()).abs() < f64::EPSILON);
		// Finer resolutions give smaller cells, hence deeper minimum zooms.
		assert!(cell_size_mercator(Resolution::Nine) < size);
	}

	#[tokio::test]
	async fn the_id_property_can_be_renamed() -> Result<()> {
		let op = build("from_h3 resolution=8 id_field=\"cell\" layer_name=\"hexes\"").await?;
		let tile = op
			.tile(&TileCoord::from_geo(LNG, LAT, 11)?)
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
		let op = build("from_h3 resolution=8").await?;
		let tile = op
			.tile(&TileCoord::from_geo(LNG, LAT, 11)?)
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
			"from_h3 resolution=8 | vector_update_properties data_source_path=\"{}\" \
			 layer_name=\"grid\" id_field_tiles=\"h3\" id_field_data=\"h3\"",
			// A Windows path is full of backslashes, and VPL reads those as escapes.
			csv.to_string_lossy().replace('\\', "\\\\")
		))
		.await?;

		let tile = op
			.tile(&TileCoord::from_geo(LNG, LAT, 11)?)
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
		let op = build("from_h3 resolution=8").await?;
		let layers = &op.tilejson().vector_layers.0;
		let layer = layers.get("grid").expect("layer declared");
		assert_eq!(layer.fields.get("h3").map(String::as_str), Some("String"));
		assert_eq!(layer.minzoom, Some(11));
		assert_eq!(layer.maxzoom, Some(MAX_ZOOM_LEVEL));
		Ok(())
	}

	#[tokio::test]
	async fn bad_arguments_are_rejected() {
		let err = format!("{:?}", build("from_h3 resolution=16").await.unwrap_err());
		assert!(err.contains("between 0 and 15"), "{err}");

		let err = format!(
			"{:?}",
			build("from_h3 resolution=8 max_cells_per_tile=0").await.unwrap_err()
		);
		assert!(err.contains("at least 1"), "{err}");

		// `bbox` and `max_zoom` are not arguments here, so either is a typo
		// rather than a narrowing — and is reported as one.
		let err = format!("{:?}", build("from_h3 resolution=8 bbox=[0,0,1,1]").await.unwrap_err());
		assert!(err.contains("bbox"), "{err}");

		let err = format!("{:?}", build("from_h3 resolution=8 max_zoom=12").await.unwrap_err());
		assert!(err.contains("max_zoom"), "{err}");
	}
}

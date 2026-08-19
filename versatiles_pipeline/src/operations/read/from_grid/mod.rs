//! # Projected square grid source
//!
//! Generates vector tiles filled with the cells of a regular grid — one polygon
//! per cell, carrying the cell's id as a string property and the coordinates of
//! its lower-left corner as numbers. Nothing is read from disk and no cell is
//! materialised in advance: the cells covering a tile are computed when that
//! tile is asked for.
//!
//! The point is joining data to it. Gridded statistics are published as tables
//! keyed on a cell id — `GRD_ID` in Eurostat's grids, `GITTER_ID_1km` in the
//! German census — so the polygons become useful once
//! `vector_update_properties` matches that column against the id here:
//!
//! ```text
//! from_grid epsg=3035 size=1000 bbox=[5.8,47.2,15.1,55.1]
//!   | vector_update_properties data_source_path="population.csv"
//!       id_field_tiles="id" id_field_data="GRD_ID"
//! ```
//!
//! ## What varies between publishers
//!
//! The id is built from the corner coordinates, and every publisher builds it
//! differently — see [`id_template`] for the formats real datasets use. The
//! default is the INSPIRE form that Eurostat, its member states and the Eurostat
//! GridMaker all emit.
//!
//! ## Zoom levels
//!
//! Cell size is fixed in the grid's CRS and does not change with zoom — that is
//! what keeps an id stable enough to join against — so at low zoom a single tile
//! would hold every cell in view. The source derives its own minimum zoom from
//! the cell size and a budget of cells per tile, and advertises nothing below
//! it.
//!
//! ## Edges
//!
//! A cell that is square in its own CRS is generally not square in a mercator
//! tile, so edges gain intermediate vertices where the projection curves them.
//! [`densify`] explains why neighbouring cells still meet exactly.

mod densify;
mod id_template;
mod lattice;
mod projection;

use std::{fmt::Debug, sync::Arc};

use anyhow::{Result, anyhow, ensure};
use async_trait::async_trait;
use geo_types::{Coord, Geometry, LineString, Polygon, coord};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{
	GeoBBox, TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream, WORLD_SIZE,
};
use versatiles_derive::context;
use versatiles_geometry::{
	feature_import::{TILE_EXTENT, render_tile},
	geo::{GeoFeature, GeoProperties, GeoValue},
};

use self::{densify::edge_vertices, id_template::IdTemplate, lattice::Lattice, projection::Projection};
use crate::{
	PipelineFactory,
	helpers::{grid_zoom::min_zoom_for_cell_size, tilejson::set_single_vector_layer},
	operations::read::traits::ReadTileSource,
	vpl::VPLNode,
};

/// Cells per tile the derived minimum zoom aims for when nothing is given.
const DEFAULT_MAX_CELLS_PER_TILE: u32 = 1024;

/// Zoom levels published above the derived minimum when nothing is given.
const DEFAULT_ZOOM_RANGE: u8 = 3;

/// Largest zoom level a tile pyramid may use.
const MAX_ZOOM: u8 = 30;

/// How far a densified edge may stray from the true curve, in tile pixels.
const DEFAULT_DENSIFY_TOLERANCE: f64 = 0.5;

/// Pixels per tile the tolerance is expressed in.
const TILE_PIXELS: f64 = 256.0;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Generates vector tiles containing the cells of a projected square grid, ready to be joined with data keyed on the cell id.
struct Args {
	/// EPSG code of the grid's coordinate reference system.
	/// Without GDAL: `3035` (ETRS89-LAEA, what European gridded statistics use),
	/// `3857` (web mercator) and `4326` (WGS84 lon/lat).
	epsg: u32,

	/// Edge length of a cell, in the CRS's own units — meters for a projected
	/// CRS, degrees for `epsg=4326`.
	/// For example: `size=1000` for the 1 km grid Eurostat publishes.
	size: f64,

	/// The area to cover, as `[west, south, east, north]` in WGS84 degrees.
	/// Required: an unbounded grid has no pyramid to derive, and at most cell
	/// sizes it is more tiles than can be written.
	bbox: [f64; 4],

	/// Where the cell with index `(0, 0)` has its lower-left corner, in CRS
	/// units. Default: `[0,0]`, which is what published grids align to.
	offset: Option<[f64; 2]>,

	/// Roughly how many cells one tile may hold. Decides the lowest zoom level
	/// this source offers. Default: `1024`.
	max_cells_per_tile: Option<u32>,

	/// Highest zoom level to generate. Defaults to three levels above the
	/// derived minimum, since further levels repeat the same cells.
	max_zoom: Option<u8>,

	/// Ready-made id format. Either `"inspire"` (default), which produces
	/// `CRS3035RES1000mN2691000E4341000`, or `"geostat"`, which produces
	/// `1kmN2689E4337`.
	id_preset: Option<String>,

	/// Id format spelled out, for a grid whose publisher uses neither preset.
	/// `{x}` and `{y}` are the lower-left corner, each taking an optional
	/// divisor and zero-padded width: `E{x/100:04}N{y/100:04}` produces
	/// `E0643N4567`, the form Dutch grid statistics use.
	/// Overrides `id_preset`.
	id_template: Option<String>,

	/// Name of the string property holding the cell id. Default: `"id"`.
	id_field: Option<String>,

	/// Names of the number properties holding the lower-left corner.
	/// Defaults: `"x"` and `"y"`, mirroring the `X_LLC` / `Y_LLC` columns
	/// Eurostat ships beside its ids.
	x_field: Option<String>,

	/// See `x_field`.
	y_field: Option<String>,

	/// How far a cell edge may stray from its true curve, in tile pixels.
	/// Only matters for a CRS whose straight lines bend in mercator; in
	/// `3857` and `4326` no vertices are added whatever this says.
	/// Default: `0.5`.
	densify_tolerance: Option<f64>,

	/// Name of the layer in the generated tiles. Default: `"grid"`.
	layer_name: Option<String>,
}

/// A [`TileSource`] that fabricates grid cells per requested tile.
pub struct Operation {
	grid: Arc<Grid>,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
}

/// Everything a tile needs, shared with the workers building tiles in parallel.
struct Grid {
	projection: Box<dyn Projection>,
	lattice: Lattice,
	id_template: IdTemplate,
	fields: Fields,
	tolerance_pixels: f64,
}

/// Property and layer names, kept together because every tile needs all of them.
#[derive(Debug, Clone)]
struct Fields {
	layer: String,
	id: String,
	x: String,
	y: String,
}

impl Debug for Operation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("from_grid")
			.field("projection", &self.grid.projection)
			.field("lattice", &self.grid.lattice)
			.field("id_template", &self.grid.id_template)
			.finish_non_exhaustive()
	}
}

impl Operation {
	#[context("Failed to build from_grid operation")]
	fn from_args(args: Args) -> Result<Self> {
		let projection = projection::for_epsg(args.epsg)?;
		ensure!(args.size > 0.0 && args.size.is_finite(), "size must be positive");

		let offset = args.offset.unwrap_or([0.0, 0.0]);
		let lattice = Lattice {
			size: args.size,
			offset,
		};

		let id_template = match (&args.id_template, args.id_preset.as_deref()) {
			(Some(template), _) => IdTemplate::parse(template)?,
			(None, Some("inspire") | None) => IdTemplate::inspire(args.epsg, args.size)?,
			(None, Some("geostat")) => IdTemplate::geostat(args.size)?,
			(None, Some(other)) => {
				return Err(anyhow!(
					"unknown id_preset '{other}': available presets are 'inspire' and 'geostat'. Use id_template= to \
					 spell out a format of your own."
				));
			}
		};
		id_template.validate_for(args.size, offset)?;

		let bbox = GeoBBox::new(args.bbox[0], args.bbox[1], args.bbox[2], args.bbox[3])?;
		let max_cells_per_tile = args.max_cells_per_tile.unwrap_or(DEFAULT_MAX_CELLS_PER_TILE);
		ensure!(max_cells_per_tile > 0, "max_cells_per_tile must be at least 1");

		let sample = sample_cell(projection.as_ref(), &lattice, &bbox);
		let cell_mercator = cell_size_mercator(projection.as_ref(), &lattice, sample);
		ensure!(
			cell_mercator.is_finite() && cell_mercator > 0.0,
			"a cell of {} units in EPSG:{} does not project into web mercator — check size and epsg",
			args.size,
			args.epsg
		);

		let min_zoom = min_zoom_for_cell_size(cell_mercator, max_cells_per_tile);
		let max_zoom = args
			.max_zoom
			.unwrap_or_else(|| min_zoom.saturating_add(DEFAULT_ZOOM_RANGE).min(MAX_ZOOM));
		ensure!(
			max_zoom >= min_zoom,
			"max_zoom ({max_zoom}) is below the zoom this grid starts at ({min_zoom}): cells of {} units are about {} \
			 meters across in web mercator, so a tile holds at most {max_cells_per_tile} of them from zoom {min_zoom} \
			 on. Raise max_zoom, raise max_cells_per_tile, or use larger cells.",
			args.size,
			cell_mercator.round()
		);

		let fields = Fields {
			layer: args.layer_name.unwrap_or_else(|| String::from("grid")),
			id: args.id_field.unwrap_or_else(|| String::from("id")),
			x: args.x_field.unwrap_or_else(|| String::from("x")),
			y: args.y_field.unwrap_or_else(|| String::from("y")),
		};

		log::info!(
			"from_grid: EPSG:{} cells of {} units (~{} m in mercator) cover zoom {min_zoom}..={max_zoom}, ids like '{}'",
			args.epsg,
			args.size,
			cell_mercator.round(),
			{
				let corner = lattice.corner(sample.0, sample.1);
				id_template.render(corner.x, corner.y)
			}
		);

		let pyramid = TilePyramid::from_geo_bbox(min_zoom, max_zoom, &bbox)?;
		let metadata = TileSourceMetadata::new(
			TileFormat::MVT,
			TileCompression::Uncompressed,
			Traversal::ANY,
			Some(pyramid),
		);

		let mut tilejson = TileJSON::default();
		tilejson.set_string("name", &fields.layer)?;
		set_single_vector_layer(
			&mut tilejson,
			&fields.layer,
			[
				(fields.id.clone(), String::from("String")),
				(fields.x.clone(), String::from("Number")),
				(fields.y.clone(), String::from("Number")),
			]
			.into_iter()
			.collect(),
			min_zoom,
			max_zoom,
		);
		metadata.update_tilejson(&mut tilejson);

		Ok(Self {
			grid: Arc::new(Grid {
				projection,
				lattice,
				id_template,
				fields,
				tolerance_pixels: args.densify_tolerance.unwrap_or(DEFAULT_DENSIFY_TOLERANCE),
			}),
			metadata,
			tilejson,
		})
	}
}

impl Grid {
	/// One tile: every cell reaching into it, clipped and encoded.
	fn build_tile(&self, coord: TileCoord) -> Result<Option<Tile>> {
		let tile_mercator = coord.to_mercator_bbox();
		let (xs, ys) = self.lattice.candidates(self.projection.as_ref(), tile_mercator);
		let tolerance = self.tolerance_meters(coord.level);

		let mut features = Vec::new();
		for iy in ys {
			for ix in xs.clone() {
				features.push(self.cell(ix, iy, tolerance));
			}
		}

		render_tile(features, &self.fields.layer, tile_mercator, TILE_EXTENT)?
			.map(|vector_tile| Tile::from_vector(vector_tile, TileFormat::MVT))
			.transpose()
	}

	/// One cell as a mercator polygon with its id and corner coordinates.
	fn cell(&self, ix: i64, iy: i64, tolerance: f64) -> GeoFeature {
		let (min, max) = (self.lattice.corner(ix, iy), self.lattice.corner(ix + 1, iy + 1));
		let corners = [
			coord! { x: min.x, y: min.y },
			coord! { x: max.x, y: min.y },
			coord! { x: max.x, y: max.y },
			coord! { x: min.x, y: max.y },
		];

		let mut ring: Vec<Coord<f64>> = Vec::new();
		for edge in 0..4 {
			ring.extend(edge_vertices(
				corners[edge],
				corners[(edge + 1) % 4],
				self.projection.as_ref(),
				tolerance,
			));
		}
		if let Some(first) = ring.first().copied() {
			ring.push(first);
		}

		let mut properties = GeoProperties::new();
		properties.insert(
			self.fields.id.clone(),
			GeoValue::from(self.id_template.render(min.x, min.y)),
		);
		properties.insert(self.fields.x.clone(), number(min.x));
		properties.insert(self.fields.y.clone(), number(min.y));

		GeoFeature {
			// The id is a string, and MVT feature ids are `u64`, so setting one
			// would offer a second, different key for the same cell.
			id: None,
			geometry: Geometry::Polygon(Polygon::new(LineString::from(ring), Vec::new())),
			properties,
		}
	}

	/// The densify tolerance for one zoom level, in mercator meters.
	fn tolerance_meters(&self, level: u8) -> f64 {
		let tile_span = WORLD_SIZE / 2f64.powi(i32::from(level));
		tile_span / TILE_PIXELS * self.tolerance_pixels
	}
}

/// A number property, kept whole where the value is whole.
///
/// Published grids ship corner coordinates as integers (`X_LLC` / `Y_LLC`), and
/// an id of `4341000` next to an `x` of `4341000.0` reads as two different
/// things.
fn number(value: f64) -> GeoValue {
	#[allow(clippy::cast_possible_truncation)]
	if value.fract().abs() < f64::EPSILON && value.abs() < 9e15 {
		GeoValue::from(value as i64)
	} else {
		GeoValue::from(value)
	}
}

/// A cell to measure the grid by: the one where mercator makes cells smallest.
///
/// Mercator stretches by `1/cos(lat)`, so cells are smallest — and densest per
/// tile — at the latitude in `bbox` closest to the equator. The same cell also
/// serves as the example id in the log line, where the grid's origin would be
/// misleading: it usually lies far outside the area asked for.
fn sample_cell(projection: &dyn Projection, lattice: &Lattice, bbox: &GeoBBox) -> (i64, i64) {
	let lat = if bbox.y_min <= 0.0 && bbox.y_max >= 0.0 {
		0.0
	} else if bbox.y_min.abs() < bbox.y_max.abs() {
		bbox.y_min
	} else {
		bbox.y_max
	};
	let lon = f64::midpoint(bbox.x_min, bbox.x_max);

	let mercator = versatiles_geometry::ext::mercator::coord_to_mercator(coord! { x: lon, y: lat });
	let source = projection.to_source(mercator);
	if !source.x.is_finite() || !source.y.is_finite() {
		return (0, 0);
	}

	#[allow(clippy::cast_possible_truncation)]
	let ix = ((source.x - lattice.offset[0]) / lattice.size).floor() as i64;
	#[allow(clippy::cast_possible_truncation)]
	let iy = ((source.y - lattice.offset[1]) / lattice.size).floor() as i64;
	(ix, iy)
}

/// The size of one cell in mercator meters, measured on an actual cell rather
/// than scaled from the nominal size — which keeps it honest for a CRS whose own
/// scale varies across the area too.
fn cell_size_mercator(projection: &dyn Projection, lattice: &Lattice, (ix, iy): (i64, i64)) -> f64 {
	let lower_left = projection.to_mercator(lattice.corner(ix, iy));
	let lower_right = projection.to_mercator(lattice.corner(ix + 1, iy));
	let upper_left = projection.to_mercator(lattice.corner(ix, iy + 1));

	let width = (lower_right.x - lower_left.x).hypot(lower_right.y - lower_left.y);
	let height = (upper_left.x - lower_left.x).hypot(upper_left.y - lower_left.y);
	width.max(height)
}

impl ReadTileSource for Operation {
	#[context("Failed to build from_grid operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, _factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized + TileSource,
	{
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
		SourceType::new_container("projected grid", "grid")
	}

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self
			.metadata
			.tile_pyramid()
			.ok_or_else(|| anyhow!("tile_pyramid not set"))
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		let grid = Arc::clone(&self.grid);

		Ok(TileStream::from_bbox_parallel(bbox, move |coord| {
			// Every input was validated when the operation was built, so a
			// failure here is a bug rather than bad data.
			grid.build_tile(coord).expect("grid cells form valid geometry")
		}))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |_coord| {
			Some(())
		}))
	}
}
crate::operations::macros::define_read_factory!("from_grid", Args, Operation);

#[cfg(test)]
mod tests {
	use versatiles_core::TileCompression::Uncompressed;
	use versatiles_geometry::vector_tile::VectorTile;

	use super::*;
	use crate::PipelineFactory;

	/// Germany, the area the 1 km INSPIRE grid is usually cut to.
	const BBOX: &str = "bbox=[5.8,47.2,15.1,55.1]";
	const LON: f64 = 13.4;
	const LAT: f64 = 52.5;

	async fn build(vpl: &str) -> Result<Box<dyn TileSource>> {
		PipelineFactory::new_dummy().operation_from_vpl(vpl).await
	}

	fn args(epsg: u32, size: f64) -> Args {
		Args {
			epsg,
			size,
			bbox: [5.8, 47.2, 15.1, 55.1],
			offset: None,
			max_cells_per_tile: None,
			max_zoom: None,
			id_preset: None,
			id_template: None,
			id_field: None,
			x_field: None,
			y_field: None,
			densify_tolerance: None,
			layer_name: None,
		}
	}

	fn features_of(tile: Tile) -> Result<Vec<GeoFeature>> {
		let vt = VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?;
		vt.layers[0].to_features()
	}

	#[tokio::test]
	async fn cell_size_decides_the_zoom_range() -> Result<()> {
		// 1 km cells are ~1.5 km across in mercator at this latitude, so 1024 of
		// them fit in a tile from zoom 10 down.
		let op = build(&format!("from_grid epsg=3035 size=1000 {BBOX}")).await?;
		let pyramid = op.tile_pyramid().await?;
		assert_eq!(pyramid.level_min(), Some(10));
		assert_eq!(pyramid.level_max(), Some(13));

		// 100 km cells are servable much earlier.
		let op = build(&format!("from_grid epsg=3035 size=100000 {BBOX}")).await?;
		assert_eq!(op.tile_pyramid().await?.level_min(), Some(4));
		Ok(())
	}

	#[tokio::test]
	async fn ids_and_corners_follow_the_inspire_form() -> Result<()> {
		let op = build(&format!("from_grid epsg=3035 size=1000 {BBOX}")).await?;
		let tile = op
			.tile(&TileCoord::from_geo(LON, LAT, 10)?)
			.await?
			.expect("tile present");
		let features = features_of(tile)?;

		assert!(!features.is_empty());
		for feature in &features {
			let Some(GeoValue::String(id)) = feature.properties.get("id") else {
				panic!("expected a string id");
			};
			let (Some(GeoValue::Int(x)), Some(GeoValue::Int(y))) =
				(feature.properties.get("x"), feature.properties.get("y"))
			else {
				panic!("expected integer corner coordinates, got {feature:?}");
			};
			assert_eq!(id, &format!("CRS3035RES1000mN{y}E{x}"));
			assert_eq!(x % 1000, 0, "corners sit on the grid");
			assert_eq!(y % 1000, 0);
		}
		Ok(())
	}

	#[test]
	fn neighbouring_cells_share_their_edge_exactly() -> Result<()> {
		let operation = Operation::from_args(args(3035, 1000.0))?;
		let grid = &operation.grid;
		let tolerance = grid.tolerance_meters(10);

		let left = grid.cell(4341, 2691, tolerance);
		let right = grid.cell(4342, 2691, tolerance);

		let bits = |feature: &GeoFeature| -> Vec<(u64, u64)> {
			let Geometry::Polygon(polygon) = &feature.geometry else {
				panic!("expected a polygon");
			};
			polygon
				.exterior()
				.coords()
				.map(|c| (c.x.to_bits(), c.y.to_bits()))
				.collect()
		};

		let (left_bits, right_bits) = (bits(&left), bits(&right));
		let shared = left_bits.iter().filter(|c| right_bits.contains(c)).count();

		// The shared edge runs from (4342000, 2691000) to (4342000, 2692000):
		// both corners plus whatever the densifier put between them, and not one
		// vertex less — a single differing bit would show as a sliver on the map.
		let edge = edge_vertices(
			grid.lattice.corner(4342, 2691),
			grid.lattice.corner(4342, 2692),
			grid.projection.as_ref(),
			tolerance,
		);
		assert_eq!(shared, edge.len() + 1);
		Ok(())
	}

	#[test]
	fn a_mercator_grid_needs_no_extra_vertices() -> Result<()> {
		let operation = Operation::from_args(args(3857, 1000.0))?;
		let cell = operation.grid.cell(0, 0, operation.grid.tolerance_meters(10));
		let Geometry::Polygon(polygon) = &cell.geometry else {
			panic!("expected a polygon");
		};
		// Four corners and the closing vertex.
		assert_eq!(polygon.exterior().coords().count(), 5);
		Ok(())
	}

	#[test]
	fn a_curved_projection_gains_vertices_when_asked() -> Result<()> {
		let mut tight = args(3035, 100_000.0);
		tight.densify_tolerance = Some(0.001);
		let operation = Operation::from_args(tight)?;
		let cell = operation.grid.cell(43, 26, operation.grid.tolerance_meters(10));
		let Geometry::Polygon(polygon) = &cell.geometry else {
			panic!("expected a polygon");
		};
		assert!(polygon.exterior().coords().count() > 5);
		Ok(())
	}

	#[tokio::test]
	async fn the_geostat_preset_and_custom_templates_work() -> Result<()> {
		let op = build(&format!("from_grid epsg=3035 size=1000 {BBOX} id_preset=\"geostat\"")).await?;
		let tile = op
			.tile(&TileCoord::from_geo(LON, LAT, 10)?)
			.await?
			.expect("tile present");
		let features = features_of(tile)?;
		let Some(GeoValue::String(id)) = features[0].properties.get("id") else {
			panic!("expected a string id");
		};
		assert!(id.starts_with("1kmN"), "{id}");

		let op = build(&format!(
			"from_grid epsg=3035 size=1000 {BBOX} id_template=\"E{{x/1000:05}}N{{y/1000:05}}\" id_field=\"cell\""
		))
		.await?;
		let tile = op
			.tile(&TileCoord::from_geo(LON, LAT, 10)?)
			.await?
			.expect("tile present");
		let features = features_of(tile)?;
		let Some(GeoValue::String(id)) = features[0].properties.get("cell") else {
			panic!("expected a string id");
		};
		assert_eq!(id.len(), 12, "{id}");
		Ok(())
	}

	/// The reason this source exists.
	#[tokio::test]
	async fn cells_can_be_joined_with_data_keyed_on_the_id() -> Result<()> {
		let op = build(&format!("from_grid epsg=3035 size=1000 {BBOX}")).await?;
		let coord = TileCoord::from_geo(LON, LAT, 10)?;
		let features = features_of(op.tile(&coord).await?.expect("tile present"))?;
		let Some(GeoValue::String(id)) = features[0].properties.get("id") else {
			panic!("expected a string id");
		};

		let dir = assert_fs::TempDir::new()?;
		let csv = dir.path().join("population.csv");
		std::fs::write(&csv, format!("GRD_ID,population\n{id},1234\n"))?;

		let joined = build(&format!(
			"from_grid epsg=3035 size=1000 {BBOX} | vector_update_properties data_source_path=\"{}\" \
			 layer_name=\"grid\" id_field_tiles=\"id\" id_field_data=\"GRD_ID\"",
			csv.display()
		))
		.await?;

		let features = features_of(joined.tile(&coord).await?.expect("tile present"))?;
		let cell = features
			.iter()
			.find(|f| f.properties.get("id") == Some(&GeoValue::from(id.clone())))
			.expect("the cell is still there");
		assert_eq!(cell.properties.get("population"), Some(&GeoValue::from(1234)));
		Ok(())
	}

	#[tokio::test]
	async fn bad_arguments_are_rejected() {
		let cases = [
			("from_grid epsg=28992 size=100", "4326"),
			("from_grid epsg=3035 size=0", "size must be positive"),
			("from_grid epsg=3035 size=1000 id_preset=\"nope\"", "unknown id_preset"),
			("from_grid epsg=3035 size=1000 id_template=\"static\"", "no {x} or {y}"),
			(
				"from_grid epsg=3035 size=100 id_preset=\"geostat\" id_template=\"{x/1000}\"",
				"share an id",
			),
			(
				"from_grid epsg=3035 size=1000 max_zoom=2",
				"below the zoom this grid starts at",
			),
		];

		for (vpl, expected) in cases {
			let err = format!("{:?}", build(&format!("{vpl} {BBOX}")).await.unwrap_err());
			assert!(err.contains(expected), "{vpl}\nexpected {expected:?} in:\n{err}");
		}

		let err = format!("{:?}", build("from_grid epsg=3035 size=1000").await.unwrap_err());
		assert!(err.contains("bbox"), "{err}");
	}

	#[tokio::test]
	async fn tilejson_declares_the_layer_and_its_fields() -> Result<()> {
		let op = build(&format!("from_grid epsg=3035 size=1000 {BBOX}")).await?;
		let layers = &op.tilejson().vector_layers.0;
		let layer = layers.get("grid").expect("layer declared");
		assert_eq!(layer.fields.get("id").map(String::as_str), Some("String"));
		assert_eq!(layer.fields.get("x").map(String::as_str), Some("Number"));
		assert_eq!(layer.fields.get("y").map(String::as_str), Some("Number"));
		Ok(())
	}
}

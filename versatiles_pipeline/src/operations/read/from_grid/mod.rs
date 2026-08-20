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
//!
//! ## Cells and tiles
//!
//! A cell reaching into several tiles is drawn in each of them, clipped to the
//! tile it is in. The same id therefore recurs across tiles, which is what a
//! join expects — but the geometry carrying it in any one tile is only the part
//! that falls inside that tile, so it is not something to measure areas from.
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
#[cfg(feature = "gdal")]
mod gdal_projection;
mod id_template;
mod lattice;
mod projection;

use std::{collections::HashMap, fmt::Debug, sync::Arc};

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

use self::{
	densify::{deviates, edge_interior},
	id_template::IdTemplate,
	lattice::Lattice,
	projection::Projection,
};
use crate::{
	PipelineFactory,
	helpers::{grid_zoom::min_zoom_for_cell_size, tilejson::set_single_vector_layer},
	vpl::VPLNode,
};

/// Cells per tile the derived minimum zoom aims for when nothing is given.
const DEFAULT_MAX_CELLS_PER_TILE: u32 = 1024;

/// Zoom levels published above the derived minimum when nothing is given.
const DEFAULT_ZOOM_RANGE: u8 = 3;

/// Largest zoom level a tile pyramid may use.
const MAX_ZOOM: u8 = 30;

/// How many parallel edges share one densification verdict.
///
/// Curvature changes slowly across a grid, so probing every edge would pay a
/// transform to learn what its neighbour already knows.
const DENSIFY_BLOCK: i64 = 64;

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
		if xs.is_empty() || ys.is_empty() {
			return Ok(None);
		}

		let block = CellBlock::build(
			self,
			*xs.start(),
			*xs.end(),
			*ys.start(),
			*ys.end(),
			self.tolerance_meters(coord.level),
		);

		render_tile(block.features(self), &self.fields.layer, tile_mercator, TILE_EXTENT)?
			.map(|vector_tile| Tile::from_vector(vector_tile, TileFormat::MVT))
			.transpose()
	}

	/// Whether the edges around `(ix, iy)` bend enough to need extra vertices.
	///
	/// Probing every edge would cost a transform per edge, which after the corner
	/// grid is the bulk of what is left. Curvature changes slowly, so one probe
	/// decides for a block of [`DENSIFY_BLOCK`] parallel edges — and the verdict
	/// is keyed on lattice indices and zoom, never on the tile, so a cell that
	/// spans a tile border is drawn the same way in both tiles.
	fn densifies(
		&self,
		edge: Edge,
		ix: i64,
		iy: i64,
		tolerance: f64,
		cache: &mut HashMap<(Edge, i64, i64), bool>,
	) -> bool {
		let (fixed, block) = match edge {
			Edge::Horizontal => (iy, ix.div_euclid(DENSIFY_BLOCK)),
			Edge::Vertical => (ix, iy.div_euclid(DENSIFY_BLOCK)),
		};

		*cache.entry((edge, fixed, block)).or_insert_with(|| {
			let start = block * DENSIFY_BLOCK;
			let (a, b) = match edge {
				Edge::Horizontal => (self.lattice.corner(start, fixed), self.lattice.corner(start + 1, fixed)),
				Edge::Vertical => (self.lattice.corner(fixed, start), self.lattice.corner(fixed, start + 1)),
			};
			deviates(
				a,
				b,
				self.projection.to_mercator(a),
				self.projection.to_mercator(b),
				self.projection.as_ref(),
				tolerance,
			)
		})
	}

	/// The densify tolerance for one zoom level, in mercator meters.
	fn tolerance_meters(&self, level: u8) -> f64 {
		let tile_span = WORLD_SIZE / 2f64.powi(i32::from(level));
		tile_span / TILE_PIXELS * self.tolerance_pixels
	}
}

/// Which way an edge runs. Horizontal edges join `(ix, iy)` to `(ix + 1, iy)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Edge {
	Horizontal,
	Vertical,
}

/// A rectangle of cells, projected once.
///
/// Corners are shared by four cells and edges by two, so projecting them per
/// cell means transforming each corner up to eight times. Here the corner grid
/// is projected in a single pass and every edge densified once, which is what
/// makes an expensive projection — GDAL's, at roughly ten times the cost of a
/// hand-written one per point — affordable.
struct CellBlock {
	/// Cells across and up.
	size: (usize, usize),
	/// Corners in the grid's CRS, row-major, `(nx + 1) * (ny + 1)` of them.
	source: Vec<Coord<f64>>,
	/// The same corners in web mercator.
	mercator: Vec<Coord<f64>>,
	/// Vertices between the ends of each horizontal edge, `nx * (ny + 1)`.
	horizontal: Vec<Vec<Coord<f64>>>,
	/// Vertices between the ends of each vertical edge, `(nx + 1) * ny`.
	vertical: Vec<Vec<Coord<f64>>>,
}

impl CellBlock {
	fn build(grid: &Grid, x0: i64, x1: i64, y0: i64, y1: i64, tolerance: f64) -> Self {
		// A candidate range is a handful of cells wide; anything that does not fit
		// a `usize` is an empty block rather than a panic.
		let nx = usize::try_from(x1 - x0 + 1).unwrap_or(0);
		let ny = usize::try_from(y1 - y0 + 1).unwrap_or(0);

		let mut source = Vec::with_capacity((nx + 1) * (ny + 1));
		for j in 0..=ny {
			for i in 0..=nx {
				source.push(grid.lattice.corner(x0 + as_offset(i), y0 + as_offset(j)));
			}
		}
		let mut mercator = source.clone();
		grid.projection.to_mercator_many(&mut mercator);

		let corner = |i: usize, j: usize| j * (nx + 1) + i;
		let mut verdicts = HashMap::new();
		let projection = grid.projection.as_ref();

		let mut horizontal = Vec::with_capacity(nx * (ny + 1));
		for j in 0..=ny {
			for i in 0..nx {
				let (a, b) = (corner(i, j), corner(i + 1, j));
				horizontal.push(
					if grid.densifies(
						Edge::Horizontal,
						x0 + as_offset(i),
						y0 + as_offset(j),
						tolerance,
						&mut verdicts,
					) {
						edge_interior(source[a], source[b], mercator[a], mercator[b], projection, tolerance)
					} else {
						Vec::new()
					},
				);
			}
		}

		let mut vertical = Vec::with_capacity((nx + 1) * ny);
		for j in 0..ny {
			for i in 0..=nx {
				let (a, b) = (corner(i, j), corner(i, j + 1));
				vertical.push(
					if grid.densifies(
						Edge::Vertical,
						x0 + as_offset(i),
						y0 + as_offset(j),
						tolerance,
						&mut verdicts,
					) {
						edge_interior(source[a], source[b], mercator[a], mercator[b], projection, tolerance)
					} else {
						Vec::new()
					},
				);
			}
		}

		Self {
			size: (nx, ny),
			source,
			mercator,
			horizontal,
			vertical,
		}
	}

	fn features(&self, grid: &Grid) -> Vec<GeoFeature> {
		let (nx, ny) = self.size;
		let mut features = Vec::with_capacity(nx * ny);
		for j in 0..ny {
			for i in 0..nx {
				features.extend(self.cell(grid, i, j));
			}
		}
		features
	}

	/// One cell as a mercator polygon with its id and corner coordinates.
	///
	/// `None` where the cell has no image in web mercator — beyond a projection's
	/// area of use, or past the mercator latitude limit. Dropping it is the only
	/// honest answer: a cell drawn from broken coordinates would land somewhere
	/// it does not belong.
	fn cell(&self, grid: &Grid, i: usize, j: usize) -> Option<GeoFeature> {
		let (nx, _) = self.size;
		let corner = |i: usize, j: usize| self.mercator[j * (nx + 1) + i];

		// Each edge is walked in the direction this cell needs. The two that run
		// against the canonical order reverse the stored vertices, which moves
		// values without arithmetic, so the neighbouring cell sharing the edge
		// ends up with the very same coordinates.
		let mut ring = vec![corner(i, j)];
		ring.extend(&self.horizontal[j * nx + i]);
		ring.push(corner(i + 1, j));
		ring.extend(&self.vertical[j * (nx + 1) + i + 1]);
		ring.push(corner(i + 1, j + 1));
		ring.extend(self.horizontal[(j + 1) * nx + i].iter().rev());
		ring.push(corner(i, j + 1));
		ring.extend(self.vertical[j * (nx + 1) + i].iter().rev());

		if ring.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
			return None;
		}
		ring.push(ring[0]);

		let origin = self.source[j * (nx + 1) + i];
		let mut properties = GeoProperties::new();
		properties.insert(
			grid.fields.id.clone(),
			GeoValue::from(grid.id_template.render(origin.x, origin.y)),
		);
		properties.insert(grid.fields.x.clone(), number(origin.x));
		properties.insert(grid.fields.y.clone(), number(origin.y));

		Some(GeoFeature {
			// The id is a string, and MVT feature ids are `u64`, so setting one
			// would offer a second, different key for the same cell.
			id: None,
			geometry: Geometry::Polygon(Polygon::new(LineString::from(ring), Vec::new())),
			properties,
		})
	}
}

/// A loop counter as a lattice offset.
#[allow(clippy::cast_possible_wrap)]
fn as_offset(i: usize) -> i64 {
	i as i64
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

impl Operation {
	#[context("Failed to build from_grid operation in VPL node {:?}", vpl_node.name)]
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
		SourceType::new_container("projected grid", "grid")
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

	/// Coarse cells and a tolerance tight enough that LAEA's curve shows up.
	fn curved_args() -> Args {
		let mut args = args(3035, 100_000.0);
		args.densify_tolerance = Some(0.001);
		args
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

		// Two cells built independently, as two neighbouring tiles would build them.
		let left = CellBlock::build(grid, 4341, 4341, 2691, 2691, tolerance)
			.cell(grid, 0, 0)
			.expect("cell projects");
		let right = CellBlock::build(grid, 4342, 4342, 2691, 2691, tolerance)
			.cell(grid, 0, 0)
			.expect("cell projects");

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
		let (from, to) = (grid.lattice.corner(4342, 2691), grid.lattice.corner(4342, 2692));
		let edge = edge_interior(
			from,
			to,
			grid.projection.to_mercator(from),
			grid.projection.to_mercator(to),
			grid.projection.as_ref(),
			tolerance,
		);
		assert_eq!(shared, edge.len() + 2);
		Ok(())
	}

	/// Within a block, the two cells either side of an edge must be handed the
	/// same vertices — the tile builder computes each edge once, so this checks
	/// the assembly walks it in the right direction from both sides.
	#[test]
	fn cells_in_one_block_share_their_edge() -> Result<()> {
		let operation = Operation::from_args(curved_args())?;
		let grid = &operation.grid;
		// A coarse grid at a low zoom, where LAEA curves enough to add vertices.
		let block = CellBlock::build(grid, 43, 44, 26, 26, grid.tolerance_meters(4));
		assert!(
			!block.vertical[1].is_empty(),
			"the shared edge should be densified here"
		);

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

		let left = bits(&block.cell(grid, 0, 0).expect("cell projects"));
		let right = bits(&block.cell(grid, 1, 0).expect("cell projects"));
		let shared = left.iter().filter(|c| right.contains(c)).count();
		assert_eq!(
			shared,
			block.vertical[1].len() + 2,
			"both corners and every vertex between them"
		);
		Ok(())
	}

	/// A cell that straddles a tile border is drawn in both tiles, from two
	/// different blocks. The two copies have to agree — which is why the
	/// densification verdict is keyed on lattice indices rather than on the tile.
	#[test]
	fn a_cell_is_the_same_from_any_block() -> Result<()> {
		let operation = Operation::from_args(curved_args())?;
		let grid = &operation.grid;
		let tolerance = grid.tolerance_meters(4);

		let alone = CellBlock::build(grid, 43, 43, 26, 26, tolerance)
			.cell(grid, 0, 0)
			.expect("cell projects");
		// The same cell as the corner of a larger block, offset differently.
		let in_block = CellBlock::build(grid, 41, 45, 24, 28, tolerance)
			.cell(grid, 2, 2)
			.expect("cell projects");

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
		assert_eq!(bits(&alone), bits(&in_block));
		assert_eq!(alone.properties, in_block.properties);
		Ok(())
	}

	#[test]
	fn a_mercator_grid_needs_no_extra_vertices() -> Result<()> {
		let operation = Operation::from_args(args(3857, 1000.0))?;
		let grid = &operation.grid;
		let cell = CellBlock::build(grid, 0, 0, 0, 0, grid.tolerance_meters(10))
			.cell(grid, 0, 0)
			.expect("cell projects");
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
		let grid = &operation.grid;
		let cell = CellBlock::build(grid, 43, 43, 26, 26, grid.tolerance_meters(10))
			.cell(grid, 0, 0)
			.expect("cell projects");
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
			// A Windows path is full of backslashes, and VPL reads those as escapes.
			csv.to_string_lossy().replace('\\', "\\\\")
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

	/// EPSG:28992 is the Dutch grid CRS: unreachable without GDAL, ordinary with it.
	#[tokio::test]
	async fn a_crs_only_gdal_knows() {
		let result = build(&format!("from_grid epsg=28992 size=100 {BBOX}")).await;

		#[cfg(feature = "gdal")]
		assert!(result.is_ok(), "{:?}", result.err());

		#[cfg(not(feature = "gdal"))]
		{
			let err = format!("{:?}", result.unwrap_err());
			assert!(err.contains("4326") && err.contains("gdal"), "{err}");
		}
	}

	/// Counts how often a tile asks the projection for a coordinate.
	#[derive(Debug)]
	struct Counting {
		inner: super::projection::Laea,
		count: Arc<std::sync::atomic::AtomicUsize>,
	}

	impl Projection for Counting {
		fn to_mercator(&self, c: Coord<f64>) -> Coord<f64> {
			self.count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
			self.inner.to_mercator(c)
		}

		fn to_source(&self, c: Coord<f64>) -> Coord<f64> {
			self.inner.to_source(c)
		}

		fn to_mercator_many(&self, coords: &mut [Coord<f64>]) {
			self.count.fetch_add(coords.len(), std::sync::atomic::Ordering::Relaxed);
			self.inner.to_mercator_many(coords);
		}
	}

	/// A grid that counts the transforms a tile costs it.
	fn counting_grid(template: &Operation) -> (Grid, Arc<std::sync::atomic::AtomicUsize>) {
		let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
		let grid = Grid {
			projection: Box::new(Counting {
				inner: super::projection::Laea::etrs89(),
				count: Arc::clone(&count),
			}),
			lattice: template.grid.lattice,
			id_template: template.grid.id_template.clone(),
			fields: template.grid.fields.clone(),
			tolerance_pixels: template.grid.tolerance_pixels,
		};
		(grid, count)
	}

	/// A corner is shared by four cells and an edge by two, so projecting per
	/// cell means transforming each corner up to eight times over. Projecting the
	/// block instead should cost barely more than one transform per cell.
	#[test]
	fn a_tile_costs_about_one_transform_per_cell() -> Result<()> {
		let template = Operation::from_args(args(3035, 1000.0))?;
		let (grid, count) = counting_grid(&template);

		let tile = grid
			.build_tile(TileCoord::from_geo(13.4, 52.5, 10)?)?
			.expect("tile present");
		let cells = VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?.layers[0]
			.to_features()?
			.len();
		let transforms = count.load(std::sync::atomic::Ordering::Relaxed);

		assert!(cells > 500, "expected a full tile of cells, got {cells}");
		// Per cell rather than in total, so the bound survives a different tile.
		let per_cell = transforms as f64 / cells as f64;
		assert!(
			per_cell < 2.0,
			"{transforms} transforms for {cells} cells is {per_cell:.2} each"
		);
		Ok(())
	}

	#[test]
	#[ignore = "measurement, not a test"]
	fn measure_transforms_per_tile() -> Result<()> {
		let template = Operation::from_args(args(3035, 1000.0))?;
		let (grid, count) = counting_grid(&template);

		let tile = grid
			.build_tile(TileCoord::from_geo(13.4, 52.5, 10)?)?
			.expect("tile present");
		let cells = VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?.layers[0]
			.to_features()?
			.len();
		let transforms = count.load(std::sync::atomic::Ordering::Relaxed);

		println!("\ntile z10 of a 1 km EPSG:3035 grid:");
		println!("  cells drawn     : {cells}");
		println!(
			"  transforms used : {transforms} ({:.2} per cell)",
			transforms as f64 / cells as f64
		);
		println!("  per-cell design : {} (12 per cell)", cells * 12);
		Ok(())
	}

	#[cfg(feature = "gdal")]
	#[test]
	#[ignore = "measurement, not a test"]
	fn measure_gdal_tile_time() -> Result<()> {
		use std::time::Instant;

		let mut rd = args(28992, 500.0);
		rd.bbox = [4.85, 52.3, 4.95, 52.4];
		let operation = Operation::from_args(rd)?;
		let coord = TileCoord::from_geo(4.9, 52.35, 11)?;

		// First tile pays for PROJ's setup in this thread; that is startup, not
		// per-tile work.
		let start = Instant::now();
		let tile = operation.grid.build_tile(coord)?.expect("tile present");
		let warmup = start.elapsed();
		let cells = VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?.layers[0]
			.to_features()?
			.len();

		const ROUNDS: u32 = 20;
		let start = Instant::now();
		for _ in 0..ROUNDS {
			std::hint::black_box(operation.grid.build_tile(coord)?);
		}
		let per_tile = start.elapsed() / ROUNDS;

		println!("\nEPSG:28992 grid, 500 m cells, tile z11:");
		println!("  cells per tile    : {cells}");
		println!("  first tile        : {warmup:?}  (includes PROJ setup)");
		println!("  steady state      : {per_tile:?} per tile");
		println!(
			"  implied rate      : {:.0} tiles/s single-threaded",
			1.0 / per_tile.as_secs_f64()
		);
		Ok(())
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

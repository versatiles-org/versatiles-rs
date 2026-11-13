use std::sync::Arc;

use anyhow::{Context, Result};
use geo::{BooleanOps, BoundingRect, MapCoords};
use geo_types::{Coord, Geometry, MultiLineString, MultiPoint, MultiPolygon, Polygon, Rect};
use versatiles_container::{Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileCoord, TilePyramid};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::{VectorTile, VectorTileFeature, VectorTileLayer};

use crate::{
	PipelineFactory,
	helpers::location::FileLocation,
	operations::{
		raster::raster_mask::{
			blur_function::BlurFunction,
			mask_geometry::{MaskGeometry, TileClassification, read_mask_polygons},
		},
		removed::refuse_removed_parameters,
		transform::TileTransform,
	},
	vpl::VPLNode,
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Cuts vector tiles to a GeoJSON polygon, keeping only what lies inside it.
///
/// Points outside the polygon are dropped, and lines and polygons are cut at its
/// edge. A point exactly on the edge counts as inside. Properties are kept as
/// they are.
///
/// ```vpl
/// from_container filename="osm.versatiles" | vector_mask geojson="berlin.geojson"
/// ```
///
/// Every layer is cut, and tiles outside the polygon are not produced at all.
/// To cut only some layers, put the mask in a `vector_branch`. This keeps place
/// names inside the region while country and state names stay on the whole map:
///
/// ```vpl
/// from_container filename="osm.versatiles"
///   | vector_branch expr="$layer == 'place_labels'" [
///       vector_mask geojson="berlin-brandenburg.geojson"
///     ]
/// ```
///
/// With `invert=true` it keeps what lies outside the polygon instead, exactly
/// what is dropped without it: a point or line on the edge is dropped, and so
/// are the tiles the polygon covers. This leaves a hole for an overlay:
///
/// ```vpl
/// from_container filename="osm.versatiles" | vector_mask geojson="city.geojson" invert=true
/// ```
///
/// Tiles entirely on the kept side pass through without being decoded, and
/// tiles entirely on the other side are dropped without being decoded, so only
/// tiles on the edge are. Geometry in a tile's buffer is cut where
/// it really lies, up to one tile beyond the tile's edge.
///
/// Cut geometry is rounded back to the tile grid, and a sliver that rounding
/// collapses is dropped.
///
/// The mask is not reprojected: coordinates outside the WGS84 range are refused,
/// as is a `crs` member naming another projection. Overlapping polygons in the
/// GeoJSON are merged first, so an area covered twice stays inside.
struct Args {
	/// Path or URL of a GeoJSON file holding a Polygon or MultiPolygon, in
	/// EPSG:4326 lon/lat degrees.
	#[vpl(accepts = "geojson, json")]
	geojson: FileLocation,
	/// Whether to keep what lies outside the polygon instead of what lies inside. Defaults to `false`.
	#[vpl(default = "false")]
	invert: Option<bool>,
}

/// How far beyond its own edge a tile's geometry is cut where it really lies.
///
/// MVT does not bound the buffer: coordinates may lie anywhere outside the
/// tile's extent. Classifying the tile on its own bbox would let a tile just
/// inside the polygon pass through with buffer geometry sticking out of it.
/// Classifying it grown by this many tile widths on each side keeps every real
/// buffer (commonly 1/64 to 1/16 of a tile) inside the classified area, at the
/// cost of decoding the tiles within one tile of the polygon's edge.
const CLASSIFY_MARGIN_TILES: f64 = 1.0;

#[derive(Debug)]
struct Operation {
	mask: Arc<MaskGeometry>,
	/// The mask's polygons in Web Mercator metres, as merged by
	/// [`read_mask_polygons`].
	polygons: MultiPolygon<f64>,
	/// The source pyramid, clipped to the mask unless inverted.
	tile_pyramid: TilePyramid,
	/// Whether what lies outside the polygon is kept instead.
	invert: bool,
}

/// What became of one feature.
enum Cut {
	/// Entirely kept: the feature stays as it was, byte for byte.
	Unchanged(VectorTileFeature),
	/// Partly kept: the feature stays with its cut geometry.
	Changed(VectorTileFeature),
	/// Entirely dropped.
	Dropped,
}

/// Converts between a tile's integer coordinates (y down) and Web Mercator metres (y up).
struct TileSpace {
	x_min: f64,
	y_max: f64,
	/// Mercator metres per tile unit.
	scale: f64,
}

impl TileSpace {
	fn new(tile_bbox: [f64; 4], extent: u32) -> Self {
		let [x_min, _, x_max, y_max] = tile_bbox;
		Self {
			x_min,
			y_max,
			scale: (x_max - x_min) / f64::from(extent),
		}
	}

	fn to_mercator(&self, c: Coord<f64>) -> Coord<f64> {
		Coord {
			x: self.x_min + c.x * self.scale,
			y: self.y_max - c.y * self.scale,
		}
	}

	fn to_tile(&self, c: Coord<f64>) -> Coord<f64> {
		Coord {
			x: (c.x - self.x_min) / self.scale,
			y: (self.y_max - c.y) / self.scale,
		}
	}

	fn rect_to_mercator(&self, rect: Rect<f64>) -> [f64; 4] {
		let a = self.to_mercator(rect.min());
		let b = self.to_mercator(rect.max());
		[a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)]
	}
}

/// A Mercator bbox grown by `tiles` of its own widths on each side.
fn grow_bbox(bbox: [f64; 4], tiles: f64) -> [f64; 4] {
	let [x_min, y_min, x_max, y_max] = bbox;
	let dx = (x_max - x_min) * tiles;
	let dy = (y_max - y_min) * tiles;
	[x_min - dx, y_min - dy, x_max + dx, y_max + dy]
}

impl Operation {
	#[context("Building vector_mask operation in VPL node {:?}", vpl_node.name)]
	async fn build_transform(
		vpl_node: VPLNode,
		source: &dyn TileSource,
		factory: &PipelineFactory,
	) -> Result<Operation> {
		refuse_removed_parameters(&vpl_node)?;
		let args = Args::from_vpl_node(&vpl_node)?;
		let geojson = args.geojson.read_all(factory).await?;
		let polygons =
			read_mask_polygons(geojson.as_slice()).with_context(|| format!("reading the mask {}", args.geojson))?;

		let mut tile_pyramid = source.tile_pyramid().await?.as_ref().clone();
		let operation = Self::new(polygons, &mut tile_pyramid, args.invert.unwrap_or(false))?;
		Ok(operation)
	}

	/// Builds the operation from mask polygons in Web Mercator metres, clipping
	/// `tile_pyramid` to the mask unless inverted.
	///
	/// The polygons must not overlap, since the clipping counts crossings:
	/// [`read_mask_polygons`] merges them.
	fn new(polygons: MultiPolygon<f64>, tile_pyramid: &mut TilePyramid, invert: bool) -> Result<Self> {
		// Not inverted itself: inverting here swaps what is kept, and keeps the
		// edge with the inside, so that the two modes split every feature.
		let mask = MaskGeometry::from_mercator(polygons.clone(), 0.0, 0.0, BlurFunction::default())?;
		if !invert && let Some(geo_bbox) = mask.geo_bbox() {
			tile_pyramid.intersect_geo_bbox(&geo_bbox)?;
		}

		Ok(Self {
			mask: Arc::new(mask),
			polygons,
			tile_pyramid: tile_pyramid.clone(),
			invert,
		})
	}

	/// Whether everything in `bbox`, in Mercator metres, is kept, dropped, or
	/// has to be looked at: [`TileClassification::FullyInside`] means kept.
	fn classify(&self, bbox: [f64; 4]) -> TileClassification {
		match (self.mask.classify_tile(bbox), self.invert) {
			(TileClassification::FullyInside, true) => TileClassification::FullyOutside,
			(TileClassification::FullyOutside, true) => TileClassification::FullyInside,
			(class, _) => class,
		}
	}

	/// Cuts a decoded tile, or returns `Ok(None)` when nothing is left of it.
	fn cut_tile(&self, coord: &TileCoord, mut tile: VectorTile) -> Result<Option<VectorTile>> {
		let tile_bbox = coord.to_mercator_bbox();
		if self.classify(grow_bbox(tile_bbox, CLASSIFY_MARGIN_TILES)) == TileClassification::FullyOutside {
			return Ok(None);
		}

		let mut kept = Vec::with_capacity(tile.layers.len());
		for mut layer in tile.layers.drain(..) {
			self.cut_layer(&mut layer, tile_bbox)?;
			if !layer.features.is_empty() {
				kept.push(layer);
			}
		}
		tile.layers = kept;

		Ok((!tile.layers.is_empty()).then_some(tile))
	}

	fn cut_layer(&self, layer: &mut VectorTileLayer, tile_bbox: [f64; 4]) -> Result<()> {
		let space = TileSpace::new(tile_bbox, layer.extent.unwrap_or(4096));
		// The mask cut down to the area around this tile, in tile coordinates.
		// Built only when a line or polygon actually needs cutting.
		let mut local_mask: Option<MultiPolygon<f64>> = None;

		let features = std::mem::take(&mut layer.features);
		let mut kept = Vec::with_capacity(features.len());
		let mut dropped_any = false;
		for feature in features {
			let cut = self
				.cut_feature(feature, &space, tile_bbox, &mut local_mask)
				.with_context(|| format!("cutting a feature in layer {:?}", layer.name))?;
			match cut {
				Cut::Unchanged(f) | Cut::Changed(f) => kept.push(f),
				Cut::Dropped => dropped_any = true,
			}
		}
		layer.features = kept;

		if dropped_any && !layer.features.is_empty() {
			// Rebuilds the property tables, so the keys and values only dropped
			// features used are not carried along.
			layer.filter_map_properties(Some)?;
		}
		Ok(())
	}

	fn cut_feature(
		&self,
		feature: VectorTileFeature,
		space: &TileSpace,
		tile_bbox: [f64; 4],
		local_mask: &mut Option<MultiPolygon<f64>>,
	) -> Result<Cut> {
		// Lenient, because a cut feature is written back: the strict decoder drops
		// the rings of a polygon wound against the spec, and that loss would end
		// up in the output, while the encoder rewinds whatever it is given.
		let geometry = feature.to_geometry_lenient()?;
		let Some(rect) = geometry.bounding_rect() else {
			// No geometry, so nothing to place inside or outside.
			return Ok(Cut::Unchanged(feature));
		};

		match self.classify(space.rect_to_mercator(rect)) {
			TileClassification::FullyInside => return Ok(Cut::Unchanged(feature)),
			TileClassification::FullyOutside => return Ok(Cut::Dropped),
			TileClassification::Partial => {}
		}

		let kept = |c: Coord<f64>| {
			let m = space.to_mercator(c);
			(self.mask.signed_distance([m.x, m.y]) >= 0.0) != self.invert
		};

		let cut: Geometry<f64> = match geometry {
			Geometry::Point(p) => {
				if !kept(p.0) {
					return Ok(Cut::Dropped);
				}
				return Ok(Cut::Unchanged(feature));
			}
			Geometry::MultiPoint(mp) => {
				let before = mp.0.len();
				let points: Vec<_> = mp.0.into_iter().filter(|p| kept(p.0)).collect();
				if points.len() == before {
					return Ok(Cut::Unchanged(feature));
				}
				Geometry::MultiPoint(MultiPoint(points))
			}
			Geometry::LineString(ls) => {
				let local = local_mask.get_or_insert_with(|| self.local_mask(space, tile_bbox));
				Geometry::MultiLineString(local.clip(&MultiLineString(vec![ls]), self.invert))
			}
			Geometry::MultiLineString(mls) => {
				let local = local_mask.get_or_insert_with(|| self.local_mask(space, tile_bbox));
				Geometry::MultiLineString(local.clip(&mls, self.invert))
			}
			Geometry::Polygon(p) => {
				let local = local_mask.get_or_insert_with(|| self.local_mask(space, tile_bbox));
				Geometry::MultiPolygon(self.cut_polygons(&MultiPolygon(vec![p]), local))
			}
			Geometry::MultiPolygon(mp) => {
				let local = local_mask.get_or_insert_with(|| self.local_mask(space, tile_bbox));
				Geometry::MultiPolygon(self.cut_polygons(&mp, local))
			}
			// The decoder produces none of the others; keep what cannot be placed.
			_ => return Ok(Cut::Unchanged(feature)),
		};

		let mut cut = VectorTileFeature::from_geometry(feature.id, feature.tag_ids, cut)?;
		cut.position = feature.position;
		// Rounding to the tile grid can collapse a sliver to nothing, and the
		// encoder then skips its rings.
		if cut.count_geometry_points() == 0 {
			return Ok(Cut::Dropped);
		}
		Ok(Cut::Changed(cut))
	}

	/// What is kept of `polygons`: the part inside `local`, or outside it when
	/// inverted.
	fn cut_polygons(&self, polygons: &MultiPolygon<f64>, local: &MultiPolygon<f64>) -> MultiPolygon<f64> {
		if self.invert {
			polygons.difference(local)
		} else {
			polygons.intersection(local)
		}
	}

	/// The mask cut to the classified area around a tile, in that tile's coordinates.
	///
	/// Cutting the whole mask against every feature would make each feature pay
	/// for every vertex of a country's border; this is paid once per edge tile
	/// and leaves only the few vertices near it.
	fn local_mask(&self, space: &TileSpace, tile_bbox: [f64; 4]) -> MultiPolygon<f64> {
		let [x_min, y_min, x_max, y_max] = grow_bbox(tile_bbox, CLASSIFY_MARGIN_TILES);
		let area = Polygon::from(Rect::new(Coord { x: x_min, y: y_min }, Coord { x: x_max, y: y_max }));
		self.polygons.intersection(&area).map_coords(|c| space.to_tile(c))
	}
}

/// Implemented directly rather than through `AsTileTransform`, so a tile entirely
/// on the kept side is handed on as the bytes it arrived as, without a decode.
impl TileTransform for Operation {
	const TAG: &'static str = "vector_mask";

	fn update_metadata(&self, metadata: &mut TileSourceMetadata) {
		metadata.set_tile_pyramid(self.tile_pyramid.clone());
	}

	#[context("Failed to run vector_mask")]
	fn run(&self, coord: &TileCoord, tile: Tile) -> Result<Option<Tile>> {
		match self.classify(grow_bbox(coord.to_mercator_bbox(), CLASSIFY_MARGIN_TILES)) {
			TileClassification::FullyInside => return Ok(Some(tile)),
			TileClassification::FullyOutside => return Ok(None),
			_ => {}
		}

		let format = tile.format();
		let Some(vector) = self.cut_tile(coord, tile.into_vector()?)? else {
			return Ok(None);
		};
		Ok(Some(Tile::from_vector(vector, format)?))
	}
}

crate::operations::macros::define_transform_factory!("vector_mask", Args, Operation, requires: Vector, per_tile);

#[cfg(test)]
mod tests {
	use geo::MapCoords;
	use geo_types::{LineString, Point, polygon};
	use versatiles_geometry::{
		ext::MercatorExt,
		geo::{GeoFeature, GeoProperties, GeoValue},
	};

	use super::*;

	/// A square from 0° to 10° in both directions, with a hole from 4° to 6°,
	/// read the way the operation reads it.
	///
	/// The hole is wound the same way as the exterior, against RFC 7946, which
	/// real GeoJSON often is too.
	fn mask_polygons() -> MultiPolygon<f64> {
		read_mask_polygons(
			br#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},
			"geometry":{"type":"Polygon","coordinates":[
				[[0,0],[10,0],[10,10],[0,10],[0,0]],
				[[4,4],[6,4],[6,6],[4,6],[4,4]]]}}]}"#,
		)
		.unwrap()
	}

	fn operation() -> Operation {
		operation_inverted(false)
	}

	fn operation_inverted(invert: bool) -> Operation {
		let mut pyramid = TilePyramid::new_full_up_to(8);
		Operation::new(mask_polygons(), &mut pyramid, invert).unwrap()
	}

	/// A z2 tile covering 0°–90° E and 0°–66.5° N, so the mask sits in its
	/// lower left corner and the tile is on the mask's edge.
	fn edge_coord() -> TileCoord {
		TileCoord::new(2, 2, 1).unwrap()
	}

	/// Converts lon/lat degrees into the coordinates of `coord`'s tile.
	fn to_tile(coord: &TileCoord, lon: f64, lat: f64) -> Coord<f64> {
		let space = TileSpace::new(coord.to_mercator_bbox(), 4096);
		space.to_tile(Point::new(lon, lat).to_mercator().0)
	}

	fn feature(geometry: Geometry<f64>, name: &str) -> GeoFeature {
		let mut f = GeoFeature::new(geometry);
		f.properties = GeoProperties::from(vec![("name", GeoValue::from(name))]);
		f
	}

	fn point(coord: &TileCoord, lon: f64, lat: f64, name: &str) -> GeoFeature {
		feature(Geometry::Point(Point(to_tile(coord, lon, lat))), name)
	}

	fn tile(layers: Vec<(&str, Vec<GeoFeature>)>) -> VectorTile {
		VectorTile::new(
			layers
				.into_iter()
				.map(|(name, features)| VectorTileLayer::from_features(name.to_string(), features, 4096, 2).unwrap())
				.collect(),
		)
	}

	/// The `name` of every feature, per layer.
	fn names(tile: &VectorTile) -> Vec<(String, Vec<String>)> {
		tile
			.layers
			.iter()
			.map(|l| {
				let names = l
					.to_features()
					.unwrap()
					.into_iter()
					.map(|f| f.properties.get("name").unwrap().to_string())
					.collect();
				(l.name.clone(), names)
			})
			.collect()
	}

	#[test]
	fn points_are_kept_inside_and_dropped_outside_and_in_the_hole() {
		let c = edge_coord();
		let input = tile(vec![(
			"places",
			vec![
				point(&c, 2.0, 2.0, "inside"),
				point(&c, 20.0, 20.0, "outside"),
				point(&c, 5.0, 5.0, "in the hole"),
				point(&c, 9.0, 8.0, "inside too"),
			],
		)]);
		let out = operation().cut_tile(&c, input).unwrap().unwrap();
		assert_eq!(
			names(&out),
			[(
				"places".to_string(),
				vec!["inside".to_string(), "inside too".to_string()]
			)]
		);
	}

	#[test]
	fn a_multipoint_keeps_only_its_points_inside() {
		let c = edge_coord();
		let points = MultiPoint(vec![
			Point(to_tile(&c, 2.0, 2.0)),
			Point(to_tile(&c, 20.0, 20.0)),
			Point(to_tile(&c, 5.0, 5.0)),
			Point(to_tile(&c, 8.0, 3.0)),
		]);
		let input = tile(vec![("places", vec![feature(Geometry::MultiPoint(points), "group")])]);
		let out = operation().cut_tile(&c, input).unwrap().unwrap();
		assert_eq!(out.layers[0].features.len(), 1);
		let Geometry::MultiPoint(kept) = out.layers[0].features[0].to_geometry().unwrap() else {
			panic!("expected a MultiPoint");
		};
		let expected = [to_tile(&c, 2.0, 2.0), to_tile(&c, 8.0, 3.0)];
		assert_eq!(kept.0.len(), expected.len());
		for (p, e) in kept.0.iter().zip(expected) {
			assert!(
				(p.x() - e.x).abs() <= 1.0 && (p.y() - e.y).abs() <= 1.0,
				"{p:?} != {e:?}"
			);
		}
	}

	#[test]
	fn a_point_on_the_edge_counts_as_inside() {
		let mask = MaskGeometry::from_mercator(mask_polygons(), 0.0, 0.0, BlurFunction::default()).unwrap();
		let on_edge = Point::new(10.0, 3.0).to_mercator();
		assert!(mask.signed_distance([on_edge.x(), on_edge.y()]) >= 0.0);
	}

	#[test]
	fn a_tile_with_nothing_left_is_dropped() {
		let c = edge_coord();
		let input = tile(vec![("places", vec![point(&c, 20.0, 20.0, "outside")])]);
		assert!(operation().cut_tile(&c, input).unwrap().is_none());
	}

	/// A cut feature is a new one, built from its cut geometry, so it is where
	/// a position could be lost. Dropping a feature also rebuilds the layer's
	/// property tables, which must not lose it either.
	#[test]
	fn features_keep_their_positions_when_cut_or_kept() {
		let c = edge_coord();
		let line = LineString(vec![to_tile(&c, 2.0, 2.0), to_tile(&c, 20.0, 2.0)]);
		let points = MultiPoint(vec![Point(to_tile(&c, 2.0, 3.0)), Point(to_tile(&c, 20.0, 3.0))]);
		let mut input = tile(vec![(
			"mixed",
			vec![
				point(&c, 2.0, 2.0, "kept"),
				point(&c, 20.0, 20.0, "dropped"),
				feature(Geometry::LineString(line), "cut line"),
				feature(Geometry::MultiPoint(points), "cut points"),
				point(&c, 9.0, 8.0, "kept too"),
			],
		)]);
		for (position, feature) in (10..).zip(&mut input.layers[0].features) {
			feature.position = Some(position);
		}

		let out = operation().cut_tile(&c, input).unwrap().unwrap();
		let positions: Vec<_> = out.layers[0].features.iter().map(|f| f.position).collect();
		assert_eq!(positions, [Some(10), Some(12), Some(13), Some(14)]);
	}

	#[test]
	fn a_line_is_cut_at_the_edge() {
		let c = edge_coord();
		let line = LineString(vec![to_tile(&c, 2.0, 2.0), to_tile(&c, 20.0, 2.0)]);
		let input = tile(vec![("streets", vec![feature(Geometry::LineString(line), "street")])]);
		let out = operation().cut_tile(&c, input).unwrap().unwrap();

		let geometry = out.layers[0].features[0].to_geometry().unwrap();
		let rect = geometry.bounding_rect().unwrap();
		let east_edge = to_tile(&c, 10.0, 2.0).x;
		assert!(
			(rect.max().x - east_edge).abs() <= 1.0,
			"{rect:?} should end at x={east_edge}"
		);
		assert!((rect.min().x - to_tile(&c, 2.0, 2.0).x).abs() <= 1.0, "{rect:?}");
	}

	#[test]
	fn a_polygon_is_cut_to_the_mask_including_its_hole() {
		let c = edge_coord();
		// A square from 3° to 20°: it covers the hole and reaches beyond the mask.
		let square = polygon![
			(x: 3.0, y: 3.0), (x: 20.0, y: 3.0), (x: 20.0, y: 20.0), (x: 3.0, y: 20.0), (x: 3.0, y: 3.0)
		]
		.map_coords(|g| Point::from(g).to_mercator().0)
		.map_coords(|m| TileSpace::new(c.to_mercator_bbox(), 4096).to_tile(m));
		let input = tile(vec![("land", vec![feature(Geometry::Polygon(square), "land")])]);
		let out = operation().cut_tile(&c, input).unwrap().unwrap();

		let Geometry::MultiPolygon(mp) = out.layers[0].features[0].to_geometry().unwrap() else {
			panic!("expected a polygon");
		};
		assert_eq!(mp.0.len(), 1);
		assert_eq!(mp.0[0].interiors().len(), 1, "the mask's hole is cut out");
		let rect = mp.bounding_rect().unwrap();
		let corner = to_tile(&c, 10.0, 10.0);
		assert!((rect.max().x - corner.x).abs() <= 1.0, "{rect:?}");
		assert!((rect.min().y - corner.y).abs() <= 1.0, "{rect:?}");
	}

	/// Converts a geometry in lon/lat degrees into the coordinates of `coord`'s tile.
	fn in_tile<G: MapCoords<f64, f64, Output = G>>(coord: &TileCoord, g: &G) -> G {
		g.map_coords(|c| to_tile(coord, c.x, c.y))
	}

	/// Cuts one feature on the edge tile and decodes what is left of it.
	fn cut_one(geometry: Geometry<f64>) -> Option<Geometry<f64>> {
		let c = edge_coord();
		let input = tile(vec![("l", vec![feature(geometry, "f")])]);
		let out = operation().cut_tile(&c, input).unwrap()?;
		Some(out.layers[0].features[0].to_geometry().unwrap())
	}

	#[test]
	fn a_line_through_the_hole_is_split_in_two() {
		let c = edge_coord();
		let line = in_tile(&c, &LineString::from(vec![(5.0, 2.0), (5.0, 8.0)]));
		let Some(Geometry::MultiLineString(mls)) = cut_one(Geometry::LineString(line)) else {
			panic!("expected lines");
		};
		assert_eq!(mls.0.len(), 2);
	}

	#[test]
	fn a_line_along_the_edge_is_kept() {
		// As a point on the edge is.
		let c = edge_coord();
		let line = in_tile(&c, &LineString::from(vec![(10.0, 2.0), (10.0, 8.0)]));
		let Some(Geometry::MultiLineString(mls)) = cut_one(Geometry::LineString(line.clone())) else {
			panic!("expected lines");
		};
		let (rect, expected) = (mls.bounding_rect().unwrap(), line.bounding_rect().unwrap());
		assert!(
			(rect.height() - expected.height()).abs() <= 1.0,
			"{rect:?} != {expected:?}"
		);
	}

	#[test]
	fn a_polygon_covering_the_tile_becomes_the_mask() {
		let c = edge_coord();
		let p = in_tile(
			&c,
			&polygon![(x: -20.0, y: -20.0), (x: 80.0, y: -20.0), (x: 80.0, y: 60.0), (x: -20.0, y: 60.0)],
		);
		let Some(Geometry::MultiPolygon(mp)) = cut_one(Geometry::Polygon(p)) else {
			panic!("expected a polygon");
		};
		assert_eq!(mp.0.len(), 1);
		assert_eq!(mp.0[0].interiors().len(), 1, "the mask's hole");
		let rect = mp.bounding_rect().unwrap();
		let corner = to_tile(&c, 10.0, 10.0);
		assert!((rect.max().x - corner.x).abs() <= 1.0, "{rect:?}");
		assert!((rect.min().y - corner.y).abs() <= 1.0, "{rect:?}");
	}

	#[test]
	fn a_polygon_keeps_its_own_hole() {
		let c = edge_coord();
		let p = in_tile(
			&c,
			&polygon!(
				exterior: [(x: 2.0, y: 2.0), (x: 20.0, y: 2.0), (x: 20.0, y: 20.0), (x: 2.0, y: 20.0)],
				interiors: [[(x: 7.0, y: 7.0), (x: 8.0, y: 7.0), (x: 8.0, y: 8.0), (x: 7.0, y: 8.0)]],
			),
		);
		let Some(Geometry::MultiPolygon(mp)) = cut_one(Geometry::Polygon(p)) else {
			panic!("expected a polygon");
		};
		assert_eq!(mp.0.len(), 1);
		assert_eq!(mp.0[0].interiors().len(), 2, "its own hole and the mask's");
	}

	#[test]
	fn a_band_across_the_hole_is_split_in_two() {
		let c = edge_coord();
		let p = in_tile(
			&c,
			&polygon![(x: 2.0, y: 4.5), (x: 8.0, y: 4.5), (x: 8.0, y: 5.5), (x: 2.0, y: 5.5)],
		);
		let Some(Geometry::MultiPolygon(mp)) = cut_one(Geometry::Polygon(p)) else {
			panic!("expected a polygon");
		};
		assert_eq!(mp.0.len(), 2);
	}

	#[test]
	fn the_pyramid_is_clipped_to_the_mask() {
		assert!(operation().tile_pyramid.count_tiles() < TilePyramid::new_full_up_to(8).count_tiles());
	}

	/// Cuts one feature on the edge tile with the inverted mask.
	fn cut_one_inverted(geometry: Geometry<f64>) -> Option<Geometry<f64>> {
		let c = edge_coord();
		let input = tile(vec![("l", vec![feature(geometry, "f")])]);
		let out = operation_inverted(true).cut_tile(&c, input).unwrap()?;
		Some(out.layers[0].features[0].to_geometry().unwrap())
	}

	#[test]
	fn inverted_points_are_kept_outside_and_in_the_hole() {
		let c = edge_coord();
		let input = tile(vec![(
			"places",
			vec![
				point(&c, 2.0, 2.0, "inside"),
				point(&c, 20.0, 20.0, "outside"),
				point(&c, 5.0, 5.0, "in the hole"),
				point(&c, 10.0, 3.0, "on the edge"),
			],
		)]);
		let out = operation_inverted(true).cut_tile(&c, input).unwrap().unwrap();
		assert_eq!(
			names(&out),
			[(
				"places".to_string(),
				vec!["outside".to_string(), "in the hole".to_string()]
			)]
		);
	}

	#[test]
	fn an_inverted_line_keeps_the_part_outside() {
		let c = edge_coord();
		let line = in_tile(&c, &LineString::from(vec![(2.0, 2.0), (20.0, 2.0)]));
		let Some(Geometry::MultiLineString(mls)) = cut_one_inverted(Geometry::LineString(line)) else {
			panic!("expected lines");
		};
		let rect = mls.bounding_rect().unwrap();
		assert!((rect.min().x - to_tile(&c, 10.0, 2.0).x).abs() <= 1.0, "{rect:?}");
		assert!((rect.max().x - to_tile(&c, 20.0, 2.0).x).abs() <= 1.0, "{rect:?}");
	}

	#[test]
	fn an_inverted_line_along_the_edge_is_dropped() {
		// As a point on the edge is: it is kept without inverting.
		let c = edge_coord();
		let line = in_tile(&c, &LineString::from(vec![(10.0, 2.0), (10.0, 8.0)]));
		assert!(cut_one_inverted(Geometry::LineString(line)).is_none());
	}

	/// Inverting splits a polygon with the plain mask: the two parts share no
	/// area and add up to the whole.
	#[test]
	fn an_inverted_polygon_is_the_rest_of_it() {
		use geo::Area;
		let c = edge_coord();
		// A square from 3° to 20°: it covers the hole and reaches beyond the mask.
		let square = in_tile(
			&c,
			&polygon![(x: 3.0, y: 3.0), (x: 20.0, y: 3.0), (x: 20.0, y: 20.0), (x: 3.0, y: 20.0)],
		);
		let Some(Geometry::MultiPolygon(inside)) = cut_one(Geometry::Polygon(square.clone())) else {
			panic!("expected a polygon");
		};
		let Some(Geometry::MultiPolygon(outside)) = cut_one_inverted(Geometry::Polygon(square.clone())) else {
			panic!("expected a polygon");
		};
		assert_eq!(outside.0.len(), 2, "the hole, and the L around the mask's corner");
		assert!(inside.intersection(&outside).unsigned_area() < 1000.0, "they overlap");
		let whole = square.unsigned_area();
		let sum = inside.unsigned_area() + outside.unsigned_area();
		// Both are rounded back to the tile grid along their edges.
		assert!((sum - whole).abs() / whole < 0.005, "{sum} != {whole}");
	}

	#[test]
	fn the_inverted_pyramid_is_not_clipped() {
		assert_eq!(
			operation_inverted(true).tile_pyramid.count_tiles(),
			TilePyramid::new_full_up_to(8).count_tiles()
		);
	}

	#[test]
	fn inverted_tiles_inside_and_outside_are_classified_without_decoding() {
		let op = operation_inverted(true);
		let blob = || {
			Tile::from_blob(
				versatiles_core::Blob::from("not a tile"),
				versatiles_core::TileCompression::Uncompressed,
				versatiles_core::TileFormat::MVT,
			)
		};
		let inside = TileCoord::from_geo(2.0, 8.0, 9).unwrap();
		assert!(op.run(&inside, blob()).unwrap().is_none(), "dropped without decoding");
		let outside = TileCoord::from_geo(50.0, 50.0, 8).unwrap();
		assert!(
			op.run(&outside, blob()).unwrap().is_some(),
			"passed through without decoding"
		);
	}

	#[test]
	fn tiles_inside_and_outside_are_classified_without_decoding() {
		let op = operation();
		// z9 tile around 2° E, 8° N: inside the mask and away from the hole, with
		// room for the one-tile margin the classification adds.
		let inside = TileCoord::from_geo(2.0, 8.0, 9).unwrap();
		let tile = Tile::from_blob(
			versatiles_core::Blob::from("not a tile"),
			versatiles_core::TileCompression::Uncompressed,
			versatiles_core::TileFormat::MVT,
		);
		assert!(
			op.run(&inside, tile).unwrap().is_some(),
			"passed through without decoding"
		);

		let outside = TileCoord::from_geo(50.0, 50.0, 8).unwrap();
		let tile = Tile::from_blob(
			versatiles_core::Blob::from("not a tile"),
			versatiles_core::TileCompression::Uncompressed,
			versatiles_core::TileFormat::MVT,
		);
		assert!(op.run(&outside, tile).unwrap().is_none(), "dropped without decoding");
	}
}

use std::{collections::HashSet, sync::Arc};

use anyhow::{Context, Result, bail};
use geo::{BooleanOps, BoundingRect, MapCoords, Orient, orient::Direction, unary_union};
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
		transform::{TileTransform, TransformOp},
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
/// Without `layer`, every layer is cut and tiles outside the polygon are not
/// produced at all. With `layer`, only the named layers are cut and the others
/// pass through everywhere, so this keeps place names inside the region while
/// country and state names stay on the whole map:
///
/// ```vpl
/// from_container filename="osm.versatiles"
///   | vector_mask geojson="berlin-brandenburg.geojson" layer=["place_labels"]
/// ```
///
/// Tiles entirely inside the polygon pass through without being decoded.
/// Without `layer`, tiles entirely outside are dropped without being decoded
/// too, so only tiles on the edge are. Geometry in a tile's buffer is cut where
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
	/// Layers to cut, for example `layer=["place_labels"]`. Defaults to every layer.
	#[vpl(layer_of_source)]
	layer: Option<Vec<String>>,
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
	/// The mask's polygons in Web Mercator metres, merged into one.
	polygons: MultiPolygon<f64>,
	/// Layers to cut, or `None` for every layer.
	layer_set: Option<HashSet<String>>,
	/// The source pyramid, clipped to the mask when every layer is cut.
	tile_pyramid: TilePyramid,
}

/// What became of one feature.
enum Cut {
	/// Entirely inside: the feature is kept as it was, byte for byte.
	Unchanged(VectorTileFeature),
	/// Partly inside: the feature is kept with its cut geometry.
	Changed(VectorTileFeature),
	/// Entirely outside.
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
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &PipelineFactory,
	) -> Result<TransformOp<Operation>> {
		let args = Args::from_vpl_node(&vpl_node)?;
		let geojson = args.geojson.read_all(factory).await?;
		let polygons =
			read_mask_polygons(geojson.as_slice()).with_context(|| format!("reading the mask {}", args.geojson))?;

		let mut tile_pyramid = source.tile_pyramid().await?.as_ref().clone();
		let operation = Self::new(&polygons, args.layer, &mut tile_pyramid)?;
		Ok(TransformOp::new(source, operation, factory.runtime()))
	}

	/// Builds the operation from mask polygons in Web Mercator metres, clipping
	/// `tile_pyramid` to the mask when every layer is cut.
	fn new(polygons: &MultiPolygon<f64>, layer: Option<Vec<String>>, tile_pyramid: &mut TilePyramid) -> Result<Self> {
		// Both the ray casting in `MaskGeometry` and the clipping below count
		// crossings, so two overlapping polygons would cancel out where they
		// overlap. Merging them first makes "covered by any polygon" the rule.
		//
		// The merge tells holes from outer rings by winding, which GeoJSON in the
		// wild often gets wrong despite RFC 7946, so the rings are wound by their
		// role first. Without that, a hole wound like its exterior is filled in.
		let polygons = unary_union(&polygons.orient(Direction::Default).0);
		if polygons.0.is_empty() {
			bail!("the mask polygons have no area");
		}

		let mask = MaskGeometry::from_mercator(polygons.clone(), 0.0, 0.0, BlurFunction::default())?;

		let layer_set = layer.map(|l| l.into_iter().collect::<HashSet<_>>());
		if layer_set.is_none() {
			// Only when everything is cut: with `layer`, the other layers are
			// still wanted outside the polygon.
			tile_pyramid.intersect_geo_bbox(&mask.geo_bbox())?;
		}

		Ok(Self {
			mask: Arc::new(mask),
			polygons,
			layer_set,
			tile_pyramid: tile_pyramid.clone(),
		})
	}

	fn applies_to(&self, layer_name: &str) -> bool {
		self.layer_set.as_ref().is_none_or(|set| set.contains(layer_name))
	}

	/// Cuts a decoded tile, or returns `Ok(None)` when nothing is left of it.
	fn cut_tile(&self, coord: &TileCoord, mut tile: VectorTile) -> Result<Option<VectorTile>> {
		let tile_bbox = coord.to_mercator_bbox();
		let outside =
			self.mask.classify_tile(grow_bbox(tile_bbox, CLASSIFY_MARGIN_TILES)) == TileClassification::FullyOutside;

		let mut kept = Vec::with_capacity(tile.layers.len());
		for mut layer in tile.layers.drain(..) {
			if !self.applies_to(&layer.name) {
				kept.push(layer);
				continue;
			}
			if outside {
				continue;
			}
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

		match self.mask.classify_tile(space.rect_to_mercator(rect)) {
			TileClassification::FullyInside => return Ok(Cut::Unchanged(feature)),
			TileClassification::FullyOutside => return Ok(Cut::Dropped),
			TileClassification::Partial => {}
		}

		let inside = |c: Coord<f64>| {
			let m = space.to_mercator(c);
			self.mask.signed_distance([m.x, m.y]) >= 0.0
		};

		let cut: Geometry<f64> = match geometry {
			Geometry::Point(p) => {
				if !inside(p.0) {
					return Ok(Cut::Dropped);
				}
				return Ok(Cut::Unchanged(feature));
			}
			Geometry::MultiPoint(mp) => {
				let before = mp.0.len();
				let points: Vec<_> = mp.0.into_iter().filter(|p| inside(p.0)).collect();
				if points.len() == before {
					return Ok(Cut::Unchanged(feature));
				}
				Geometry::MultiPoint(MultiPoint(points))
			}
			Geometry::LineString(ls) => {
				let local = local_mask.get_or_insert_with(|| self.local_mask(space, tile_bbox));
				Geometry::MultiLineString(local.clip(&MultiLineString(vec![ls]), false))
			}
			Geometry::MultiLineString(mls) => {
				let local = local_mask.get_or_insert_with(|| self.local_mask(space, tile_bbox));
				Geometry::MultiLineString(local.clip(&mls, false))
			}
			Geometry::Polygon(p) => {
				let local = local_mask.get_or_insert_with(|| self.local_mask(space, tile_bbox));
				Geometry::MultiPolygon(p.intersection(local))
			}
			Geometry::MultiPolygon(mp) => {
				let local = local_mask.get_or_insert_with(|| self.local_mask(space, tile_bbox));
				Geometry::MultiPolygon(mp.intersection(local))
			}
			// The decoder produces none of the others; keep what cannot be placed.
			_ => return Ok(Cut::Unchanged(feature)),
		};

		let cut = VectorTileFeature::from_geometry(feature.id, feature.tag_ids, cut)?;
		// Rounding to the tile grid can collapse a sliver to nothing, and the
		// encoder then skips its rings.
		if cut.count_geometry_points() == 0 {
			return Ok(Cut::Dropped);
		}
		Ok(Cut::Changed(cut))
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
/// inside the polygon is handed on as the bytes it arrived as, without a decode.
impl TileTransform for Operation {
	const TAG: &'static str = "vector_mask";

	fn update_metadata(&self, metadata: &mut TileSourceMetadata) {
		metadata.set_tile_pyramid(self.tile_pyramid.clone());
	}

	#[context("Failed to run vector_mask")]
	fn run(&self, coord: &TileCoord, tile: Tile) -> Result<Option<Tile>> {
		match self
			.mask
			.classify_tile(grow_bbox(coord.to_mercator_bbox(), CLASSIFY_MARGIN_TILES))
		{
			TileClassification::FullyInside => return Ok(Some(tile)),
			// Dropping only some layers needs to know which layers the tile has.
			TileClassification::FullyOutside if self.layer_set.is_none() => return Ok(None),
			_ => {}
		}

		let format = tile.format();
		let Some(vector) = self.cut_tile(coord, tile.into_vector()?)? else {
			return Ok(None);
		};
		Ok(Some(Tile::from_vector(vector, format)?))
	}
}

crate::operations::macros::define_transform_factory!("vector_mask", Args, Operation, requires: Vector);

#[cfg(test)]
mod tests {
	use geo::MapCoords;
	use geo_types::{LineString, Point, polygon};
	use versatiles_geometry::{
		ext::MercatorExt,
		geo::{GeoFeature, GeoProperties, GeoValue},
	};

	use super::*;

	/// A square from 0° to 10° in both directions, with a hole from 4° to 6°.
	///
	/// The hole is wound the same way as the exterior, against RFC 7946, which
	/// real GeoJSON often is too.
	fn mask_polygons() -> MultiPolygon<f64> {
		MultiPolygon(vec![polygon!(
			exterior: [(x: 0.0, y: 0.0), (x: 10.0, y: 0.0), (x: 10.0, y: 10.0), (x: 0.0, y: 10.0), (x: 0.0, y: 0.0)],
			interiors: [[(x: 4.0, y: 4.0), (x: 6.0, y: 4.0), (x: 6.0, y: 6.0), (x: 4.0, y: 6.0), (x: 4.0, y: 4.0)]],
		)])
		.to_mercator()
	}

	fn operation(layer: Option<&[&str]>) -> Operation {
		let mut pyramid = TilePyramid::new_full_up_to(8);
		Operation::new(
			&mask_polygons(),
			layer.map(|l| l.iter().map(|s| (*s).to_string()).collect()),
			&mut pyramid,
		)
		.unwrap()
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
		let out = operation(None).cut_tile(&c, input).unwrap().unwrap();
		assert_eq!(
			names(&out),
			[(
				"places".to_string(),
				vec!["inside".to_string(), "inside too".to_string()]
			)]
		);
	}

	#[test]
	fn a_point_on_the_edge_counts_as_inside() {
		let mask = MaskGeometry::from_mercator(mask_polygons(), 0.0, 0.0, BlurFunction::default()).unwrap();
		let on_edge = Point::new(10.0, 3.0).to_mercator();
		assert!(mask.signed_distance([on_edge.x(), on_edge.y()]) >= 0.0);
	}

	#[test]
	fn layers_outside_the_selection_pass_through() {
		let c = edge_coord();
		let input = tile(vec![
			("places", vec![point(&c, 20.0, 20.0, "outside")]),
			("boundary_labels", vec![point(&c, 20.0, 20.0, "country")]),
		]);
		let out = operation(Some(&["places"])).cut_tile(&c, input).unwrap().unwrap();
		assert_eq!(
			names(&out),
			[("boundary_labels".to_string(), vec!["country".to_string()])]
		);
	}

	#[test]
	fn a_tile_with_nothing_left_is_dropped() {
		let c = edge_coord();
		let input = tile(vec![("places", vec![point(&c, 20.0, 20.0, "outside")])]);
		assert!(operation(None).cut_tile(&c, input).unwrap().is_none());
	}

	#[test]
	fn a_line_is_cut_at_the_edge() {
		let c = edge_coord();
		let line = LineString(vec![to_tile(&c, 2.0, 2.0), to_tile(&c, 20.0, 2.0)]);
		let input = tile(vec![("streets", vec![feature(Geometry::LineString(line), "street")])]);
		let out = operation(None).cut_tile(&c, input).unwrap().unwrap();

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
		let out = operation(None).cut_tile(&c, input).unwrap().unwrap();

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

	#[test]
	fn the_pyramid_is_clipped_only_when_every_layer_is_cut() {
		assert!(operation(None).tile_pyramid.count_tiles() < TilePyramid::new_full_up_to(8).count_tiles());
		assert_eq!(
			operation(Some(&["places"])).tile_pyramid.count_tiles(),
			TilePyramid::new_full_up_to(8).count_tiles()
		);
	}

	#[test]
	fn tiles_inside_and_outside_are_classified_without_decoding() {
		let op = operation(None);
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

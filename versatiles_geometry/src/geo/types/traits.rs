use anyhow::Result;
use std::fmt::Debug;
use versatiles_core::json::JsonValue;

/// Defines the basic interface for geometric primitives, providing common functionality
/// for all geometry types.
pub trait GeometryTrait: Debug + Clone + Sized {
	/// Returns the geometric area of the geometry.
	/// For non-area geometries (e.g., points or lines), this returns 0.
	fn area(&self) -> f64;

	/// Verifies the geometric validity of the geometry.
	/// For example, checks if there are enough points or if polygons are properly closed.
	/// Returns an error if the geometry is invalid.
	fn verify(&self) -> Result<()>;

	/// Converts the geometry into a JSON representation of its coordinates.
	/// Optionally rounds coordinate values to the given precision.
	fn to_coord_json(&self, precision: Option<u8>) -> JsonValue;

	/// Checks if a point is inside this geometry.
	///
	/// For closed geometries (rings, polygons, multi-polygons), returns `true` if the
	/// point is inside the geometry. For non-closed geometries (points, lines),
	/// returns `false`.
	///
	/// Points exactly on the boundary may return either value.
	fn contains_point(&self, x: f64, y: f64) -> bool;

	/// Transform this geometry from WGS84 to Web Mercator coordinates.
	///
	/// Each coordinate is treated as (longitude, latitude) in degrees and
	/// converted to Web Mercator (EPSG:3857) coordinates in meters.
	fn to_mercator(&self) -> Self;

	/// Compute the bounding box of this geometry.
	///
	/// Returns `Some([x_min, y_min, x_max, y_max])` representing the bounding box
	/// of all coordinates in the geometry, or `None` if the geometry is empty.
	fn compute_bounds(&self) -> Option<[f64; 4]>;
}

/// Represents geometries that can be wrapped into a corresponding multi-geometry.
/// For example, a single `PointGeometry` can be converted into a `MultiPointGeometry`.
pub trait SingleGeometryTrait<Multi>: Debug + Clone {
	/// Converts the single geometry into its multi-geometry equivalent.
	fn into_multi(self) -> Multi;
}

/// Represents composite geometries that are collections of simpler elements.
/// For example, a polygon is made of rings, and a multilinestring is made of lines.
pub trait CompositeGeometryTrait<Item>: Debug + Clone {
	/// Creates a new, empty composite geometry.
	fn new() -> Self;

	/// Returns an immutable reference to the inner collection of elements.
	fn as_vec(&self) -> &Vec<Item>;

	/// Returns a mutable reference to the inner collection of elements.
	fn as_mut_vec(&mut self) -> &mut Vec<Item>;

	/// Consumes the composite geometry and returns the inner collection of elements.
	fn into_inner(self) -> Vec<Item>;

	/// Returns an iterator over owned elements of the composite geometry.
	fn into_iter(self) -> impl Iterator<Item = Item> {
		self.into_inner().into_iter()
	}

	/// Splits the composite geometry into its first element and the rest, if available.
	fn into_first_and_rest(self) -> Option<(Item, Vec<Item>)> {
		let mut iter = self.into_iter();
		iter.next().map(|first| (first, iter.collect()))
	}

	/// Checks whether the composite geometry contains no elements.
	fn is_empty(&self) -> bool {
		self.as_vec().is_empty()
	}

	/// Returns the number of elements contained in the composite geometry.
	fn len(&self) -> usize {
		self.as_vec().len()
	}

	/// Adds a new element to the composite geometry.
	fn push(&mut self, item: Item) {
		self.as_mut_vec().push(item);
	}

	/// Removes and returns the last element from the composite geometry, if any.
	fn pop(&mut self) -> Option<Item> {
		self.as_mut_vec().pop()
	}

	/// Returns a reference to the first element, if any.
	fn first(&self) -> Option<&Item> {
		self.as_vec().first()
	}

	/// Returns a reference to the last element, if any.
	fn last(&self) -> Option<&Item> {
		self.as_vec().last()
	}

	/// Returns a mutable reference to the first element, if any.
	fn first_mut(&mut self) -> Option<&mut Item> {
		self.as_mut_vec().first_mut()
	}

	/// Returns a mutable reference to the last element, if any.
	fn last_mut(&mut self) -> Option<&mut Item> {
		self.as_mut_vec().last_mut()
	}
}

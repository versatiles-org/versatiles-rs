//! A geographic crop, optionally widened by a ring of border tiles.
//!
//! Cropping a tileset is two rules that have to agree: which tiles survive, and
//! which bounds get advertised. Getting them out of step is not a visible
//! failure — the tiles are written and the bounds look plausible — so the two
//! halves live here together rather than being spelled out at each call site.

use anyhow::Result;

use crate::{GeoBBox, TilePyramid};

/// A geographic crop, optionally widened by a ring of border tiles.
///
/// `border` is a count of tiles *per zoom level*, kept around the crop. Unlike
/// the crop itself it **widens** the result: tiles in the ring lie outside
/// `bbox`, which is why [`bounds`](Self::bounds) has to extend past `bbox` when
/// a border is set — otherwise a client selecting tiles by bbox overlap would
/// never ask for the ring that was just written.
///
/// # Example
///
/// ```
/// use versatiles_core::{GeoBBox, GeoCrop, TilePyramid};
///
/// let crop = GeoCrop::new(GeoBBox::new(13.3, 52.4, 13.5, 52.6).unwrap(), 2);
///
/// let mut pyramid = TilePyramid::new_full_up_to(12);
/// crop.apply_to_pyramid(&mut pyramid).unwrap();
///
/// // Bounds cover the border tiles, so they reach past the crop itself.
/// let bounds = crop.bounds(None, &pyramid);
/// assert!(bounds.x_min < 13.3);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeoCrop {
	/// The crop rectangle in WGS84 degrees.
	pub bbox: GeoBBox,
	/// Ring of extra tiles kept around `bbox`, per zoom level. `0` is no ring.
	pub border: u32,
}

impl GeoCrop {
	/// A crop with a border ring of `border` tiles per zoom level.
	#[must_use]
	pub fn new(bbox: GeoBBox, border: u32) -> Self {
		GeoCrop { bbox, border }
	}

	/// A crop with no border ring.
	#[must_use]
	pub fn new_exact(bbox: GeoBBox) -> Self {
		GeoCrop { bbox, border: 0 }
	}

	/// `true` when this crop widens rather than only narrowing.
	#[must_use]
	pub fn has_border(&self) -> bool {
		self.border > 0
	}

	/// Restricts `pyramid` to this crop.
	///
	/// The ring is grown from a *full* pyramid cropped to `bbox`, spanning the
	/// same level range as `pyramid`, and only then intersected with `pyramid`.
	/// Buffering `pyramid` after cropping it would be subtly wrong: at low zoom
	/// levels a two-tile ring covers the entire level — level 1 has four tiles —
	/// so the ring would escape whatever coverage the source actually has, and
	/// the advertised bounds computed from it would balloon towards the whole
	/// world.
	///
	/// # Errors
	/// Returns an error if `bbox` cannot be projected onto the tile grid.
	pub fn apply_to_pyramid(&self, pyramid: &mut TilePyramid) -> Result<()> {
		if !self.has_border() {
			pyramid.intersect_geo_bbox(&self.bbox)?;
			return Ok(());
		}

		let mut ring = TilePyramid::new_full();
		if let Some(level_min) = pyramid.level_min() {
			ring.set_level_min(level_min);
		}
		if let Some(level_max) = pyramid.level_max() {
			ring.set_level_max(level_max);
		}
		ring.intersect_geo_bbox(&self.bbox)?;
		ring.buffer(self.border);

		pyramid.intersect_pyramid(&ring);
		Ok(())
	}

	/// The bounds to advertise for `cropped`, the pyramid this crop produced.
	///
	/// `existing` is whatever the source advertised, narrowed to the crop. When
	/// a border is set the result is then widened to the box touching every tile
	/// actually kept, so the ring is requested rather than merely stored.
	///
	/// Callers should also clear any advertised centre: it may no longer lie
	/// inside the result.
	#[must_use]
	pub fn bounds(&self, existing: Option<GeoBBox>, cropped: &TilePyramid) -> GeoBBox {
		let mut bounds = match existing {
			Some(mut existing) => {
				existing.intersect(&self.bbox);
				existing
			}
			None => self.bbox,
		};

		if self.has_border()
			&& let Some(touching) = cropped.geo_bbox_touching_all_tiles()
		{
			bounds.extend(&touching);
		}

		bounds
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn bbox() -> GeoBBox {
		GeoBBox::new(13.3, 52.4, 13.5, 52.6).unwrap()
	}

	fn cropped(border: u32) -> (GeoCrop, TilePyramid) {
		let crop = GeoCrop::new(bbox(), border);
		let mut pyramid = TilePyramid::new_full_up_to(12);
		crop.apply_to_pyramid(&mut pyramid).unwrap();
		(crop, pyramid)
	}

	#[test]
	fn constructors_agree() {
		assert_eq!(GeoCrop::new(bbox(), 0), GeoCrop::new_exact(bbox()));
		assert!(!GeoCrop::new_exact(bbox()).has_border());
		assert!(GeoCrop::new(bbox(), 1).has_border());
	}

	#[test]
	fn a_border_only_ever_adds_tiles() {
		let (_, exact) = cropped(0);
		let (_, bordered) = cropped(2);

		assert!(
			bordered.count_tiles() > exact.count_tiles(),
			"{} vs {}",
			exact.count_tiles(),
			bordered.count_tiles()
		);

		for level in 0..=12u8 {
			let inner = exact.level_ref(level).to_bbox();
			if inner.is_empty() {
				continue;
			}
			assert!(
				bordered.level_ref(level).to_bbox().includes_bbox(&inner),
				"level {level}: a border must not drop tiles the crop kept"
			);
		}
	}

	/// The ring is intersected with the source, so it can never reach tiles the
	/// source does not have. This is what keeps a border local for a regional
	/// container — the common case.
	#[test]
	fn a_border_is_clamped_by_the_source() {
		// A source covering roughly Brandenburg, as a regional container would.
		let mut pyramid = TilePyramid::new_full_up_to(12);
		pyramid
			.intersect_geo_bbox(&GeoBBox::new(12.0, 51.5, 15.0, 53.5).unwrap())
			.unwrap();

		let crop = GeoCrop::new(bbox(), 2);
		crop.apply_to_pyramid(&mut pyramid).unwrap();
		let bounds = crop.bounds(None, &pyramid);

		assert!(
			bounds.x_min > 11.0 && bounds.x_max < 16.0,
			"longitude escaped: {bounds:?}"
		);
		assert!(
			bounds.y_min > 50.0 && bounds.y_max < 55.0,
			"latitude escaped: {bounds:?}"
		);
	}

	/// Documents the surprising-but-correct case: against a source that covers
	/// the whole world, a two-tile ring at level 1 *is* the whole level — there
	/// are only four tiles there. Those tiles get written, so the advertised
	/// bounds have to cover them. `convert --bbox-border` behaves the same way;
	/// a border is only meaningful where the crop is small relative to the level.
	#[test]
	fn a_border_on_a_world_source_covers_whole_low_levels() {
		let (crop, pyramid) = cropped(2);
		let bounds = crop.bounds(None, &pyramid);

		assert!(
			bounds.x_min < -80.0 && bounds.x_max > 80.0,
			"a world source cannot clamp the ring: {bounds:?}"
		);
	}

	#[test]
	fn bounds_widen_with_a_border_and_not_without() {
		let (exact_crop, exact) = cropped(0);
		let (border_crop, bordered) = cropped(2);

		let exact_bounds = exact_crop.bounds(None, &exact);
		let border_bounds = border_crop.bounds(None, &bordered);

		assert_eq!(exact_bounds, bbox(), "without a border the crop is advertised as-is");
		assert!(border_bounds.x_min < exact_bounds.x_min, "west must extend");
		assert!(border_bounds.y_min < exact_bounds.y_min, "south must extend");
		assert!(border_bounds.x_max > exact_bounds.x_max, "east must extend");
		assert!(border_bounds.y_max > exact_bounds.y_max, "north must extend");
	}

	#[test]
	fn bounds_narrow_an_existing_advertisement() {
		let (crop, pyramid) = cropped(0);
		let wider = GeoBBox::new(0.0, 40.0, 20.0, 60.0).unwrap();

		assert_eq!(
			crop.bounds(Some(wider), &pyramid),
			bbox(),
			"existing bounds are narrowed"
		);
	}
}

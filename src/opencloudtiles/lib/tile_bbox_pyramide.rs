use super::{TileBBox, TileCoord2, TileCoord3};
use std::fmt;

const MAX_ZOOM_LEVEL: u64 = 32;

#[derive(Clone)]
pub struct TileBBoxPyramide {
	level_bbox: [TileBBox; MAX_ZOOM_LEVEL as usize],
}

#[allow(dead_code)]
impl TileBBoxPyramide {
	pub fn new_full() -> TileBBoxPyramide {
		TileBBoxPyramide {
			level_bbox: std::array::from_fn(|z| TileBBox::new_full(z as u64)),
		}
	}
	pub fn new_empty() -> TileBBoxPyramide {
		TileBBoxPyramide {
			level_bbox: std::array::from_fn(|_z| TileBBox::new_empty()),
		}
	}
	pub fn limit_by_geo_bbox(&mut self, geo_bbox: &[f32; 4]) {
		for (level, bbox) in self.level_bbox.iter_mut().enumerate() {
			bbox.intersect_bbox(&TileBBox::from_geo(level as u64, geo_bbox));
		}
	}
	pub fn intersect(&mut self, other_bbox_pyramide: &TileBBoxPyramide) {
		for (level, bbox) in self.level_bbox.iter_mut().enumerate() {
			let other_bbox = other_bbox_pyramide.get_level_bbox(level as u64);
			bbox.intersect_bbox(other_bbox);
		}
	}
	pub fn get_level_bbox(&self, level: u64) -> &TileBBox {
		&self.level_bbox[level as usize]
	}
	pub fn set_level_bbox(&mut self, level: u64, bbox: TileBBox) {
		self.level_bbox[level as usize] = bbox;
	}
	pub fn include_coord(&mut self, coord: &TileCoord3) {
		self.level_bbox[coord.z as usize].include_tile(coord.x, coord.y);
	}
	pub fn include_bbox(&mut self, level: u64, bbox: &TileBBox) {
		self.level_bbox[level as usize].union_bbox(bbox);
	}
	pub fn iter_levels(&self) -> impl Iterator<Item = (u64, &TileBBox)> {
		self
			.level_bbox
			.iter()
			.enumerate()
			.filter_map(|(level, bbox)| {
				if bbox.is_empty() {
					None
				} else {
					Some((level as u64, bbox))
				}
			})
	}
	pub fn iter_tile_indexes(&self) -> impl Iterator<Item = TileCoord3> + '_ {
		return self.level_bbox.iter().enumerate().flat_map(|(z, bbox)| {
			bbox
				.iter_coords()
				.map(move |TileCoord2 { x, y }| TileCoord3 { x, y, z: z as u64 })
		});
	}
	pub fn get_zoom_min(&self) -> Option<u64> {
		self
			.level_bbox
			.iter()
			.enumerate()
			.find(|(_level, bbox)| !bbox.is_empty())
			.map(|(level, _bbox)| level as u64)
	}
	pub fn get_zoom_max(&self) -> Option<u64> {
		self
			.level_bbox
			.iter()
			.enumerate()
			.rev()
			.find(|(_level, bbox)| !bbox.is_empty())
			.map(|(level, _bbox)| level as u64)
	}
	pub fn set_zoom_min(&mut self, zoom_level_min: u64) {
		for (index, bbox) in self.level_bbox.iter_mut().enumerate() {
			let level = index as u64;
			if level < zoom_level_min {
				bbox.set_empty();
			}
		}
	}
	pub fn set_zoom_max(&mut self, zoom_level_max: u64) {
		for (index, bbox) in self.level_bbox.iter_mut().enumerate() {
			let level = index as u64;
			if level > zoom_level_max {
				bbox.set_empty();
			}
		}
	}
	pub fn count_tiles(&self) -> u64 {
		return self.level_bbox.iter().map(|bbox| bbox.count_tiles()).sum();
	}
	pub fn is_empty(&self) -> bool {
		self.level_bbox.iter().all(|bbox| bbox.is_empty())
	}
	pub fn is_full(&self) -> bool {
		self
			.level_bbox
			.iter()
			.enumerate()
			.all(|(i, bbox)| bbox.is_full(i as u64))
	}
}

impl fmt::Debug for TileBBoxPyramide {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_list()
			.entries(
				self
					.iter_levels()
					.map(|(level, bbox)| format!("{}: {:?}", level, bbox)),
			)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn intersection_tests() {
		let mut pyramide1 = TileBBoxPyramide::new_empty();
		pyramide1.intersect(&TileBBoxPyramide::new_empty());
		assert!(pyramide1.is_empty());

		let mut pyramide1 = TileBBoxPyramide::new_full();
		pyramide1.intersect(&TileBBoxPyramide::new_empty());
		assert!(pyramide1.is_empty());

		let mut pyramide1 = TileBBoxPyramide::new_empty();
		pyramide1.intersect(&TileBBoxPyramide::new_full());
		assert!(pyramide1.is_empty());

		let mut pyramide1 = TileBBoxPyramide::new_full();
		pyramide1.intersect(&TileBBoxPyramide::new_full());
		assert!(pyramide1.is_full());
	}

	#[test]
	fn level_bbox() {
		let test = |z0: u64, z1: u64| {
			let mut pyramide = TileBBoxPyramide::new_empty();
			let bbox = TileBBox::new_full(z0);
			pyramide.set_level_bbox(z1, bbox.clone());
			assert_eq!(pyramide.get_level_bbox(z1).clone(), bbox);
		};

		test(0, 1);
		test(0, 30);
		test(30, 30);
	}

	#[test]
	fn zoom_min_max() {
		let test = |z0: u64, z1: u64| {
			let mut pyramide = TileBBoxPyramide::new_full();
			pyramide.set_zoom_min(z0);
			pyramide.set_zoom_max(z1);
			assert_eq!(pyramide.get_zoom_min().unwrap(), z0);
			assert_eq!(pyramide.get_zoom_max().unwrap(), z1);
		};

		test(0, 1);
		test(0, 30);
		test(30, 30);
	}
}

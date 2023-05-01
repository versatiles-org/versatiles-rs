use super::{TileBBox, TileCoord3};
use std::array::from_fn;
use std::fmt;

const MAX_ZOOM_LEVEL: u8 = 32;

#[derive(Clone, Eq)]
pub struct TileBBoxPyramide {
	level_bbox: [TileBBox; MAX_ZOOM_LEVEL as usize],
}

impl TileBBoxPyramide {
	pub fn new_full() -> TileBBoxPyramide {
		TileBBoxPyramide {
			level_bbox: from_fn(|z| TileBBox::new_full(z as u8)),
		}
	}
	pub fn new_empty() -> TileBBoxPyramide {
		TileBBoxPyramide {
			level_bbox: from_fn(|_z| TileBBox::new_empty()),
		}
	}
	pub fn limit_by_geo_bbox(&mut self, geo_bbox: &[f32; 4]) {
		for (level, bbox) in self.level_bbox.iter_mut().enumerate() {
			bbox.intersect_bbox(&TileBBox::from_geo(geo_bbox, level as u8));
		}
	}
	pub fn intersect(&mut self, other_bbox_pyramide: &TileBBoxPyramide) {
		for (level, bbox) in self.level_bbox.iter_mut().enumerate() {
			let other_bbox = other_bbox_pyramide.get_level_bbox(level as u8);
			bbox.intersect_bbox(other_bbox);
		}
	}
	pub fn get_level_bbox(&self, level: u8) -> &TileBBox {
		&self.level_bbox[level as usize]
	}
	pub fn set_level_bbox(&mut self, level: u8, bbox: TileBBox) {
		self.level_bbox[level as usize] = bbox;
	}
	pub fn include_coord(&mut self, coord: &TileCoord3) {
		self.level_bbox[coord.z as usize].include_tile(coord.x, coord.y);
	}
	pub fn include_bbox(&mut self, level: u8, bbox: &TileBBox) {
		self.level_bbox[level as usize].union_bbox(bbox);
	}
	pub fn iter_levels(&self) -> impl Iterator<Item = (u8, &TileBBox)> {
		self.level_bbox.iter().enumerate().filter_map(|(level, bbox)| {
			if bbox.is_empty() {
				None
			} else {
				Some((level as u8, bbox))
			}
		})
	}
	pub fn get_zoom_min(&self) -> Option<u8> {
		self
			.level_bbox
			.iter()
			.enumerate()
			.find(|(_level, bbox)| !bbox.is_empty())
			.map(|(level, _bbox)| level as u8)
	}
	pub fn get_zoom_max(&self) -> Option<u8> {
		self
			.level_bbox
			.iter()
			.enumerate()
			.rev()
			.find(|(_level, bbox)| !bbox.is_empty())
			.map(|(level, _bbox)| level as u8)
	}
	pub fn set_zoom_min(&mut self, zoom_level_min: u8) {
		for (index, bbox) in self.level_bbox.iter_mut().enumerate() {
			if (index as u8) < zoom_level_min {
				bbox.set_empty();
			}
		}
	}
	pub fn set_zoom_max(&mut self, zoom_level_max: u8) {
		for (index, bbox) in self.level_bbox.iter_mut().enumerate() {
			if (index as u8) > zoom_level_max {
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
	#[cfg(test)]
	pub fn is_full(&self) -> bool {
		self
			.level_bbox
			.iter()
			.enumerate()
			.all(|(i, bbox)| bbox.is_full(i as u64))
	}
	pub fn get_geo_bbox(&self) -> [f32; 4] {
		let level = self.get_zoom_max().unwrap();

		self.get_level_bbox(level).as_geo_bbox(level)
	}
}

impl fmt::Debug for TileBBoxPyramide {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_list()
			.entries(self.iter_levels().map(|(level, bbox)| format!("{level}: {bbox:?}")))
			.finish()
	}
}

impl PartialEq for TileBBoxPyramide {
	fn eq(&self, other: &Self) -> bool {
		for i in 0..MAX_ZOOM_LEVEL {
			let bbox0 = self.get_level_bbox(i);
			let bbox1 = other.get_level_bbox(i);
			if bbox0.is_empty() != bbox1.is_empty() {
				return false;
			}
			if bbox0.is_empty() {
				continue;
			}
			if bbox0 != bbox1 {
				return false;
			}
		}
		true
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
	fn limit_by_geo_bbox() {
		let mut pyramide = TileBBoxPyramide::new_full();
		pyramide.set_zoom_max(8);
		pyramide.limit_by_geo_bbox(&[8.0653f32, 51.3563f32, 12.3528f32, 52.2564f32]);

		assert_eq!(pyramide.get_level_bbox(0), &TileBBox::new(0, 0, 0, 0));
		assert_eq!(pyramide.get_level_bbox(1), &TileBBox::new(1, 0, 1, 0));
		assert_eq!(pyramide.get_level_bbox(2), &TileBBox::new(2, 1, 2, 1));
		assert_eq!(pyramide.get_level_bbox(3), &TileBBox::new(4, 2, 4, 2));
		assert_eq!(pyramide.get_level_bbox(4), &TileBBox::new(8, 5, 8, 5));
		assert_eq!(pyramide.get_level_bbox(5), &TileBBox::new(16, 10, 17, 10));
		assert_eq!(pyramide.get_level_bbox(6), &TileBBox::new(33, 21, 34, 21));
		assert_eq!(pyramide.get_level_bbox(7), &TileBBox::new(66, 42, 68, 42));
		assert_eq!(pyramide.get_level_bbox(8), &TileBBox::new(133, 84, 136, 85));
	}

	#[test]
	fn include_coord() {
		let mut pyramide = TileBBoxPyramide::new_empty();
		pyramide.include_coord(&TileCoord3::new(1, 2, 3));
		pyramide.include_coord(&TileCoord3::new(4, 5, 3));
		pyramide.include_coord(&TileCoord3::new(6, 7, 8));

		assert!(pyramide.get_level_bbox(0).is_empty());
		assert!(pyramide.get_level_bbox(1).is_empty());
		assert!(pyramide.get_level_bbox(2).is_empty());
		assert_eq!(pyramide.get_level_bbox(3), &TileBBox::new(1, 2, 4, 5));
		assert!(pyramide.get_level_bbox(4).is_empty());
		assert!(pyramide.get_level_bbox(5).is_empty());
		assert!(pyramide.get_level_bbox(6).is_empty());
		assert!(pyramide.get_level_bbox(7).is_empty());
		assert_eq!(pyramide.get_level_bbox(8), &TileBBox::new(6, 7, 6, 7));
		assert!(pyramide.get_level_bbox(9).is_empty());
	}

	#[test]
	fn include_bbox() {
		let mut pyramide = TileBBoxPyramide::new_empty();
		pyramide.include_bbox(4, &TileBBox::new(1, 2, 3, 4));
		pyramide.include_bbox(4, &TileBBox::new(5, 6, 7, 8));

		assert!(pyramide.get_level_bbox(0).is_empty());
		assert!(pyramide.get_level_bbox(1).is_empty());
		assert!(pyramide.get_level_bbox(2).is_empty());
		assert!(pyramide.get_level_bbox(3).is_empty());
		assert_eq!(pyramide.get_level_bbox(4), &TileBBox::new(1, 2, 7, 8));
		assert!(pyramide.get_level_bbox(5).is_empty());
		assert!(pyramide.get_level_bbox(6).is_empty());
		assert!(pyramide.get_level_bbox(7).is_empty());
		assert!(pyramide.get_level_bbox(8).is_empty());
		assert!(pyramide.get_level_bbox(9).is_empty());
	}

	#[test]
	fn level_bbox() {
		let test = |z0: u8, z1: u8| {
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
		let test = |z0: u8, z1: u8| {
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

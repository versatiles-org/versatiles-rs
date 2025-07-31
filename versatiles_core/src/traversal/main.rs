use crate::{
	TileBBox, TileBBoxPyramid,
	traversal::{TraversalOrder, TraversalSize},
};
use anyhow::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Traversal {
	order: TraversalOrder,
	size: TraversalSize,
}

impl Traversal {
	pub fn new(order: TraversalOrder, min_size: u32, max_size: u32) -> Result<Traversal> {
		Ok(Traversal {
			order,
			size: TraversalSize::new(min_size, max_size)?,
		})
	}

	pub fn new_any_size(min_size: u32, max_size: u32) -> Result<Traversal> {
		Ok(Traversal {
			order: TraversalOrder::AnyOrder,
			size: TraversalSize::new(min_size, max_size)?,
		})
	}

	pub const fn new_any() -> Self {
		const {
			Traversal {
				order: TraversalOrder::AnyOrder,
				size: TraversalSize::new_default(),
			}
		}
	}

	pub fn get_max_size(&self) -> Result<u32> {
		self.size.get_max_size()
	}

	pub fn order(&self) -> &TraversalOrder {
		&self.order
	}

	pub fn intersect(&mut self, other: &Traversal) -> Result<()> {
		self.size.intersect(&other.size)?;
		self.order.intersect(&other.order)
	}

	pub fn get_intersected(&self, other: &Traversal) -> Result<Traversal> {
		let mut result = self.clone();
		result.intersect(other)?;
		Ok(result)
	}

	pub fn traverse_pyramid(&self, pyramid: &TileBBoxPyramid) -> Result<Vec<TileBBox>> {
		let size = self.get_max_size()?;
		let mut bboxes: Vec<TileBBox> = pyramid.level_bbox.iter().flat_map(|b| b.iter_bbox_grid(size)).collect();
		bboxes = self.order.sort_bboxes(bboxes, size);
		Ok(bboxes)
	}
}

impl Default for Traversal {
	fn default() -> Self {
		Traversal::new_any()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::GeoBBox;
	use enumset::EnumSet;

	#[test]
	fn test_traverse_pyramid() {
		fn test(traversal_order: TraversalOrder, size: u32, bbox: [i16; 4], min_level: u8, max_level: u8) -> Vec<String> {
			let pyramid = TileBBoxPyramid::from_geo_bbox(min_level, max_level, &GeoBBox::from(&bbox));
			let traversal = Traversal {
				order: traversal_order,
				size: TraversalSize::new(1, size).unwrap(),
			};
			traversal
				.traverse_pyramid(&pyramid)
				.unwrap()
				.iter()
				.map(|b| b.as_string())
				.collect()
		}

		use TraversalOrder::*;
		for order in EnumSet::all() {
			match order {
				AnyOrder => {
					assert_eq!(
						test(order, 256, [-180, -90, 180, 90], 0, 5),
						[
							"0:[0,0,0,0]",
							"1:[0,0,1,1]",
							"2:[0,0,3,3]",
							"3:[0,0,7,7]",
							"4:[0,0,15,15]",
							"5:[0,0,31,31]"
						]
					);
					assert_eq!(
						test(order, 16, [-180, -90, 180, 90], 4, 5),
						[
							"4:[0,0,15,15]",
							"5:[0,0,15,15]",
							"5:[16,0,31,15]",
							"5:[0,16,15,31]",
							"5:[16,16,31,31]"
						]
					);
				}
				DepthFirst => {
					assert_eq!(
						test(order, 16, [-170, -60, 160, 70], 4, 6),
						[
							"6:[1,14,15,15]",
							"6:[16,14,31,15]",
							"6:[1,16,15,31]",
							"6:[16,16,31,31]",
							"5:[0,7,15,15]",
							"6:[32,14,47,15]",
							"6:[48,14,60,15]",
							"6:[32,16,47,31]",
							"6:[48,16,60,31]",
							"5:[16,7,30,15]",
							"6:[1,32,15,45]",
							"6:[16,32,31,45]",
							"5:[0,16,15,22]",
							"6:[32,32,47,45]",
							"6:[48,32,60,45]",
							"5:[16,16,30,22]",
							"4:[0,3,15,11]",
						]
					);
					assert_eq!(
						test(order, 32, [-170, -60, 160, 70], 4, 6),
						[
							"6:[1,14,31,31]",
							"6:[32,14,60,31]",
							"6:[1,32,31,45]",
							"6:[32,32,60,45]",
							"5:[0,7,30,22]",
							"4:[0,3,15,11]"
						]
					);
					assert_eq!(
						test(order, 256, [-170, -60, 160, 70], 6, 10),
						[
							"10:[28,229,255,255]",
							"10:[256,229,511,255]",
							"10:[28,256,255,511]",
							"10:[256,256,511,511]",
							"9:[14,114,255,255]",
							"10:[512,229,767,255]",
							"10:[768,229,967,255]",
							"10:[512,256,767,511]",
							"10:[768,256,967,511]",
							"9:[256,114,483,255]",
							"10:[28,512,255,726]",
							"10:[256,512,511,726]",
							"9:[14,256,255,363]",
							"10:[512,512,767,726]",
							"10:[768,512,967,726]",
							"9:[256,256,483,363]",
							"8:[7,57,241,181]",
							"7:[3,28,120,90]",
							"6:[1,14,60,45]"
						]
					)
				}
				PMTiles => {
					assert_eq!(
						test(order, 64, [-170, -60, 160, 70], 6, 8),
						[
							"6:[1,14,60,45]",
							"7:[3,28,63,63]",
							"7:[3,64,63,90]",
							"7:[64,64,120,90]",
							"7:[64,28,120,63]",
							"8:[7,57,63,63]",
							"8:[64,57,127,63]",
							"8:[64,64,127,127]",
							"8:[7,64,63,127]",
							"8:[7,128,63,181]",
							"8:[64,128,127,181]",
							"8:[128,128,191,181]",
							"8:[192,128,241,181]",
							"8:[192,64,241,127]",
							"8:[128,64,191,127]",
							"8:[128,57,191,63]",
							"8:[192,57,241,63]"
						]
					);
					assert_eq!(
						test(order, 128, [-170, -60, 160, 70], 6, 8),
						[
							"6:[1,14,60,45]",
							"7:[3,28,120,90]",
							"8:[7,57,127,127]",
							"8:[7,128,127,181]",
							"8:[128,128,241,181]",
							"8:[128,57,241,127]"
						]
					)
				}
			}
		}
	}
}

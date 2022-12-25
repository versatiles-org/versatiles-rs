use super::tile_bbox::TileBBox;

const MAX_ZOOM_LEVEL: usize = 32;
pub struct TileBBoxPyramide {
	level_bbox: Vec<TileBBox>,
}
impl TileBBoxPyramide {
	pub fn new() -> TileBBoxPyramide {
		return TileBBoxPyramide {
			level_bbox: (0..=MAX_ZOOM_LEVEL)
				.map(|l| TileBBox::new_full(l as u64))
				.collect(),
		};
	}
	pub fn intersect_level_bbox(
		&mut self,
		zoom_level: u64,
		col_min: u64,
		row_min: u64,
		col_max: u64,
		row_max: u64,
	) {
		self.level_bbox[zoom_level as usize]
			.intersect(&TileBBox::new(col_min, row_min, col_max, row_max));
	}
	pub fn limit_zoom_levels(&mut self, zoom_level_min: u64, zoom_level_max: u64) {
		for (index, bbox) in self.level_bbox.iter_mut().enumerate() {
			let level = index as u64;
			if (level < zoom_level_min) || (level > zoom_level_max) {
				bbox.set_empty();
			}
		}
	}
	pub fn limit_by_geo_bbox(&mut self, geo_bbox: [f32; 4]) {
		for (level, bbox) in self.level_bbox.iter_mut().enumerate() {
			bbox.intersect(&TileBBox::from_geo(level as u64, geo_bbox));
		}
	}
	pub fn intersect(&mut self, level_bbox: &TileBBoxPyramide) {
		for (level, bbox) in self.level_bbox.iter_mut().enumerate() {
			bbox.intersect(level_bbox.get_level_bbox(level as u64));
		}
	}
	pub fn get_level_bbox(&self, level: u64) -> &TileBBox {
		return &self.level_bbox[level as usize];
	}
}

use super::abstract_classes;
use crate::opencloudtiles::compress::*;
use clap::ValueEnum;
use std::f32::consts::PI;

#[derive(Clone)]
pub struct TileBBox {
	col_min: u64,
	row_min: u64,
	col_max: u64,
	row_max: u64,
}

impl TileBBox {
	pub fn new(col_min: u64, row_min: u64, col_max: u64, row_max: u64) -> Self {
		TileBBox {
			col_min,
			row_min,
			col_max,
			row_max,
		}
	}
	pub fn new_full(level: u64) -> Self {
		let max = 2u64.pow(level as u32);
		TileBBox::new(0, 0, max, max)
	}
	pub fn new_empty() -> Self {
		TileBBox::new(1, 1, 0, 0)
	}
	pub fn set_empty(&mut self) {
		self.col_min = 1;
		self.row_min = 1;
		self.col_max = 0;
		self.row_max = 0;
	}
	pub fn from_geo(level: u64, geo_bbox: [f32; 4]) -> Self {
		let zoom: f32 = 2.0f32.powi(level as i32);
		let x_min = zoom * (geo_bbox[0] / 360.0 + 0.5);
		let y_min = zoom * (PI - ((geo_bbox[1] / 90.0 + 1.0) * PI / 4.0).tan().ln());
		let x_max = zoom * (geo_bbox[2] / 360.0 + 0.5);
		let y_max = zoom * (PI - ((geo_bbox[3] / 90.0 + 1.0) * PI / 4.0).tan().ln());
		return TileBBox::new(x_min as u64, y_min as u64, x_max as u64, y_max as u64);
	}
	pub fn include_tile(&mut self, col: u64, row: u64) {
		if self.col_min > col {
			self.col_min = col
		}
		if self.row_min > row {
			self.row_min = row
		}
		if self.col_max < col {
			self.col_max = col
		}
		if self.row_max < row {
			self.row_max = row
		}
	}
	pub fn intersect(&mut self, bbox: &TileBBox) {
		self.col_min = self.col_min.max(bbox.col_min);
		self.row_min = self.row_min.max(bbox.row_min);
		self.col_max = self.col_max.min(bbox.col_max);
		self.row_max = self.row_max.min(bbox.row_max);
	}
	pub fn is_empty(&self) -> bool {
		return (self.col_max < self.col_min) || (self.row_max < self.row_min);
	}
	pub fn as_tuple(&self) -> (u64, u64, u64, u64) {
		return (self.col_min, self.row_min, self.col_max, self.row_max);
	}
}

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

#[derive(PartialEq, Clone, Debug, ValueEnum)]
pub enum TileFormat {
	PBF,
	PBFGzip,
	PBFBrotli,
	PNG,
	JPG,
	WEBP,
}

pub type TileData = Vec<u8>;

pub struct TileReaderParameters {
	zoom_min: u64,
	zoom_max: u64,
	level_bbox: TileBBoxPyramide,
	tile_format: TileFormat,
}

impl TileReaderParameters {
	pub fn new(
		zoom_min: u64,
		zoom_max: u64,
		tile_format: TileFormat,
		level_bbox: TileBBoxPyramide,
	) -> TileReaderParameters {
		return TileReaderParameters {
			zoom_min,
			zoom_max,
			tile_format,
			level_bbox,
		};
	}
	pub fn get_zoom_min(&self) -> u64 {
		return self.zoom_min;
	}
	pub fn get_zoom_max(&self) -> u64 {
		return self.zoom_max;
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		return &self.tile_format;
	}
	pub fn get_level_bbox(&self) -> &TileBBoxPyramide {
		return &self.level_bbox;
	}
}

pub struct TileReaderWrapper<'a> {
	reader: &'a Box<dyn abstract_classes::TileReader>,
}

impl TileReaderWrapper<'_> {
	pub fn new(reader: &Box<dyn abstract_classes::TileReader>) -> TileReaderWrapper {
		return TileReaderWrapper { reader };
	}
	pub fn get_tile_data(&self, level: u64, col: u64, row: u64) -> Option<TileData> {
		return self.reader.get_tile_data(level, col, row);
	}
}

unsafe impl Send for TileReaderWrapper<'_> {}
unsafe impl Sync for TileReaderWrapper<'_> {}

pub struct TileConverterConfig {
	zoom_min: Option<u64>,
	zoom_max: Option<u64>,
	geo_bbox: Option<[f32; 4]>,
	tile_format: Option<TileFormat>,
	level_bbox: TileBBoxPyramide,
	tile_converter: Option<fn(&TileData) -> TileData>,
	force_recompress: bool,
	finalized: bool,
}

impl TileConverterConfig {
	pub fn from_options(
		zoom_min: &Option<u64>,
		zoom_max: &Option<u64>,
		geo_bbox: &Option<Vec<f32>>,
		tile_format: &Option<TileFormat>,
		force_recompress: &bool,
	) -> Self {
		return TileConverterConfig {
			zoom_min: zoom_min.clone(),
			zoom_max: zoom_max.clone(),
			tile_format: tile_format.clone(),
			geo_bbox: geo_bbox.as_ref().map(|v| v.as_slice().try_into().unwrap()),
			level_bbox: TileBBoxPyramide::new(),
			tile_converter: None,
			force_recompress: *force_recompress,
			finalized: false,
		};
	}
	pub fn finalize_with_parameters(&mut self, parameters: &TileReaderParameters) {
		let zoom_min = parameters.get_zoom_min();
		if self.zoom_min.is_none() {
			self.zoom_min = Some(zoom_min);
		} else {
			self.zoom_min = Some(self.zoom_min.unwrap().max(zoom_min));
		}

		let zoom_max = parameters.get_zoom_max();
		if self.zoom_max.is_none() {
			self.zoom_max = Some(zoom_max);
		} else {
			self.zoom_max = Some(self.zoom_max.unwrap().min(zoom_max));
		}

		self.level_bbox.intersect(parameters.get_level_bbox());
		self.level_bbox.limit_zoom_levels(zoom_min, zoom_max);

		if self.geo_bbox.is_some() {
			self.level_bbox.limit_by_geo_bbox(self.geo_bbox.unwrap());
		}

		self.tile_converter = Some(self.calc_tile_converter(&parameters.get_tile_format()));

		self.finalized = true;
	}
	fn calc_tile_converter(&mut self, src_tile_format: &TileFormat) -> fn(&TileData) -> TileData {
		if self.tile_format.is_none() {
			self.tile_format = Some(src_tile_format.clone());
			return tile_same;
		}

		let dst_tile_format = self.tile_format.as_ref().unwrap();

		if src_tile_format == dst_tile_format {
			return tile_same;
		}

		//if src_tile_format == TileFormat::PNG | TileFormat::JPG | TileFormat::WEBP {}

		return match (src_tile_format, dst_tile_format) {
			(TileFormat::PNG, TileFormat::PNG) => tile_same,
			(TileFormat::PNG, _) => panic!(),

			(TileFormat::JPG, TileFormat::JPG) => tile_same,
			(TileFormat::JPG, _) => panic!(),

			(TileFormat::WEBP, TileFormat::WEBP) => tile_same,
			(TileFormat::WEBP, _) => panic!(),

			(TileFormat::PBF, TileFormat::PBF) => tile_same,
			(TileFormat::PBF, TileFormat::PBFBrotli) => compress_brotli,
			(TileFormat::PBF, TileFormat::PBFGzip) => compress_gzip,
			(TileFormat::PBF, _) => panic!(),

			(TileFormat::PBFBrotli, TileFormat::PBF) => decompress_brotli,
			(TileFormat::PBFBrotli, TileFormat::PBFBrotli) => {
				if self.force_recompress {
					fn tile_unbrotli_brotli(tile: &TileData) -> TileData {
						compress_brotli(&decompress_brotli(&tile))
					}
					tile_unbrotli_brotli
				} else {
					tile_same
				}
			}
			(TileFormat::PBFBrotli, TileFormat::PBFGzip) => {
				fn tile_unbrotli_gzip(tile: &TileData) -> TileData {
					compress_gzip(&decompress_brotli(&tile))
				}
				tile_unbrotli_gzip
			}
			(TileFormat::PBFBrotli, _) => panic!(),

			(TileFormat::PBFGzip, TileFormat::PBF) => decompress_gzip,
			(TileFormat::PBFGzip, TileFormat::PBFBrotli) => {
				fn tile_ungzip_brotli(tile: &TileData) -> TileData {
					compress_brotli(&&decompress_gzip(&tile))
				}
				tile_ungzip_brotli
			}
			(TileFormat::PBFGzip, TileFormat::PBFGzip) => {
				if self.force_recompress {
					fn tile_ungzip_gzip(tile: &TileData) -> TileData {
						compress_gzip(&decompress_gzip(&tile))
					}
					tile_ungzip_gzip
				} else {
					tile_same
				}
			}
			(TileFormat::PBFGzip, _) => todo!(),
		};

		fn tile_same(tile: &TileData) -> TileData {
			return tile.clone();
		}
	}
	pub fn get_tile_converter(&self) -> fn(&TileData) -> TileData {
		if !self.finalized {
			panic!()
		}

		return self.tile_converter.unwrap();
	}
	pub fn get_zoom_min(&self) -> u64 {
		if !self.finalized {
			panic!()
		}

		return self.zoom_min.unwrap();
	}
	pub fn get_zoom_max(&self) -> u64 {
		if !self.finalized {
			panic!()
		}

		return self.zoom_max.unwrap();
	}
	pub fn get_zoom_bbox(&self, zoom: u64) -> &TileBBox {
		if !self.finalized {
			panic!()
		}
		return self.level_bbox.get_level_bbox(zoom);
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		if !self.finalized {
			panic!()
		}

		return self.tile_format.as_ref().unwrap();
	}
}

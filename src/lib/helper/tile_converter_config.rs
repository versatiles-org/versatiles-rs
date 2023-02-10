use super::{DataConverter, Precompression, TileBBoxPyramide, TileFormat, TileReaderParameters};

pub struct TileConverterConfig {
	tile_format: Option<TileFormat>,
	tile_precompression: Option<Precompression>,
	tile_recompressor: Option<DataConverter>,
	compressor: Option<DataConverter>,
	bbox_pyramide: TileBBoxPyramide,
	force_recompress: bool,
	finalized: bool,
}

#[allow(dead_code)]
impl TileConverterConfig {
	pub fn new(
		tile_format: Option<TileFormat>, tile_precompression: Option<Precompression>,
		bbox_pyramide: TileBBoxPyramide, force_recompress: bool,
	) -> Self {
		TileConverterConfig {
			tile_format,
			tile_precompression,
			bbox_pyramide,
			tile_recompressor: None,
			compressor: None,
			force_recompress,
			finalized: false,
		}
	}
	pub fn finalize_with_parameters(&mut self, parameters: &TileReaderParameters) {
		self.bbox_pyramide.intersect(parameters.get_bbox_pyramide());

		self
			.tile_format
			.get_or_insert(parameters.get_tile_format().clone());
		self
			.tile_precompression
			.get_or_insert(*parameters.get_tile_precompression());

		let src_form = parameters.get_tile_format();
		let src_comp = parameters.get_tile_precompression();
		let dst_form = self.tile_format.as_ref().unwrap();
		let dst_comp = self.tile_precompression.as_ref().unwrap();
		let force_recompress = self.force_recompress;

		self.tile_recompressor = Some(DataConverter::new_tile_recompressor(
			src_form,
			src_comp,
			dst_form,
			dst_comp,
			force_recompress,
		));

		self.compressor = Some(DataConverter::new_compressor(dst_comp));

		self.finalized = true;
	}
	pub fn get_tile_recompressor(&self) -> &DataConverter {
		self.tile_recompressor.as_ref().unwrap()
	}
	pub fn get_compressor(&self) -> &DataConverter {
		self.compressor.as_ref().unwrap()
	}
	pub fn get_bbox_pyramide(&self) -> &TileBBoxPyramide {
		&self.bbox_pyramide
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		self.tile_format.as_ref().unwrap()
	}
	pub fn get_tile_precompression(&self) -> &Precompression {
		self.tile_precompression.as_ref().unwrap()
	}
}

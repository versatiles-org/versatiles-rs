use crate::shared::{Compression, DataConverter, TileBBoxPyramid, TileFormat, TilesReaderParameters};

#[derive(Debug)]
pub struct TilesConverterConfig {
	tile_format: Option<TileFormat>,
	tile_compression: Option<Compression>,
	tile_recompressor: Option<DataConverter>,
	compressor: Option<DataConverter>,
	bbox_pyramid: TileBBoxPyramid,
	force_recompress: bool,
	finalized: bool,
}

impl TilesConverterConfig {
	pub fn new(
		tile_format: Option<TileFormat>, tile_compression: Option<Compression>, bbox_pyramid: TileBBoxPyramid,
		force_recompress: bool,
	) -> Self {
		TilesConverterConfig {
			tile_format,
			tile_compression,
			bbox_pyramid,
			tile_recompressor: None,
			compressor: None,
			force_recompress,
			finalized: false,
		}
	}
	#[cfg(test)]
	pub fn new_full() -> Self {
		Self::new(None, None, TileBBoxPyramid::new_full(), false)
	}
	pub fn finalize_with_parameters(&mut self, parameters: &TilesReaderParameters) {
		let mut bbox_pyramid = parameters.bbox_pyramid.clone();
		parameters.transform_forward(&mut bbox_pyramid);

		self.bbox_pyramid.intersect(&bbox_pyramid);

		self.tile_format.get_or_insert(parameters.tile_format);
		self.tile_compression.get_or_insert(parameters.tile_compression);

		let src_form = &parameters.tile_format;
		let src_comp = &parameters.tile_compression;
		let dst_form = self.tile_format.as_ref().unwrap();
		let dst_comp = self.tile_compression.as_ref().unwrap();
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
	pub fn get_bbox_pyramid(&self) -> &TileBBoxPyramid {
		&self.bbox_pyramid
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		self.tile_format.as_ref().unwrap()
	}
	pub fn get_tile_compression(&self) -> &Compression {
		self.tile_compression.as_ref().unwrap()
	}
}

#[cfg(test)]
mod tests {
	use super::{Compression, TileBBoxPyramid, TileFormat, TilesConverterConfig, TilesReaderParameters};

	#[test]
	fn test() {
		let pyramid = TileBBoxPyramid::new_full();
		let parameters = TilesReaderParameters::new(TileFormat::PNG, Compression::Gzip, pyramid.clone());

		let mut config =
			TilesConverterConfig::new(Some(TileFormat::JPG), Some(Compression::Brotli), pyramid.clone(), true);

		config.finalize_with_parameters(&parameters);

		assert_eq!(config.get_tile_format(), &TileFormat::JPG);
		assert_eq!(config.get_tile_compression(), &Compression::Brotli);

		assert_eq!(config.get_tile_recompressor().as_string(), "UnGzip, Png2Jpg, Brotli");
		assert_eq!(config.get_compressor().as_string(), "Brotli");
		assert_eq!(config.get_bbox_pyramid(), &pyramid);
	}
}

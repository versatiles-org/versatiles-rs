mod filter;
mod raster;
mod read;
mod vector;

use crate::traits::{ReadOperationFactoryTrait, TransformOperationFactoryTrait};

pub fn get_transform_operation_factories() -> Vec<Box<dyn TransformOperationFactoryTrait>> {
	vec![
		Box::new(filter::filter_bbox::Factory {}),
		Box::new(raster::raster_overview::Factory {}),
		Box::new(vector::vector_update_properties::Factory {}),
		Box::new(vector::vector_filter_layers::Factory {}),
		Box::new(vector::vector_filter_properties::Factory {}),
	]
}

pub fn get_read_operation_factories() -> Vec<Box<dyn ReadOperationFactoryTrait>> {
	vec![
		Box::new(read::from_container::Factory {}),
		Box::new(read::from_debug::Factory {}),
		Box::new(read::from_overlayed::Factory {}),
		Box::new(read::from_overlayed_imagetiles::Factory {}),
		Box::new(read::from_merged_vectortiles::Factory {}),
		#[cfg(feature = "gdal")]
		Box::new(read::from_gdal::raster::Factory {}),
	]
}

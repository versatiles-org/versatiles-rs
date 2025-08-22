mod general;
mod raster;
mod read;
mod vector;

use crate::traits::{ReadOperationFactoryTrait, TransformOperationFactoryTrait};

pub fn get_transform_operation_factories() -> Vec<Box<dyn TransformOperationFactoryTrait>> {
	vec![
		Box::new(general::filter::Factory {}),
		Box::new(general::meta_update::Factory {}),
		Box::new(raster::raster_flatten::Factory {}),
		Box::new(raster::raster_format::Factory {}),
		Box::new(raster::raster_levels::Factory {}),
		Box::new(raster::raster_overscale::Factory {}),
		Box::new(raster::raster_overview::Factory {}),
		Box::new(vector::vector_filter_layers::Factory {}),
		Box::new(vector::vector_filter_properties::Factory {}),
		Box::new(vector::vector_update_properties::Factory {}),
	]
}

pub fn get_read_operation_factories() -> Vec<Box<dyn ReadOperationFactoryTrait>> {
	vec![
		Box::new(read::from_container::Factory {}),
		Box::new(read::from_debug::Factory {}),
		Box::new(read::from_stacked::Factory {}),
		Box::new(read::from_stacked_raster::Factory {}),
		Box::new(read::from_merged_vector::Factory {}),
		#[cfg(feature = "gdal")]
		Box::new(read::from_gdal::raster::Factory {}),
	]
}

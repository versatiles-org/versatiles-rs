//! The built-in VPL operations.
//!
//! Each operation is a module whose `Args` struct derives `VPLDecode`. The doc
//! comments on that struct and its fields *are* the operation reference — see
//! `DOCSTYLE.md` at the crate root for what they have to look like and where
//! each one is rendered.

mod dem;
mod general;
pub(crate) mod macros;
mod raster;
mod read;
pub(crate) mod transform;
mod vector;

use crate::factory::{ReadOperationFactoryTrait, TransformOperationFactoryTrait};

pub fn transform_operation_factories() -> Vec<Box<dyn TransformOperationFactoryTrait>> {
	vec![
		Box::new(general::filter::Factory {}),
		Box::new(general::meta_update::Factory {}),
		Box::new(general::remap_coords::Factory {}),
		Box::new(dem::dem_overview::Factory {}),
		Box::new(dem::dem_quantize::Factory {}),
		Box::new(dem::dem_tile_resize::Factory {}),
		Box::new(raster::raster_flatten::Factory {}),
		Box::new(raster::raster_format::Factory {}),
		Box::new(raster::raster_levels::Factory {}),
		Box::new(raster::raster_mask::Factory {}),
		Box::new(raster::raster_overscale::Factory {}),
		Box::new(raster::raster_overview::Factory {}),
		Box::new(raster::raster_tile_resize::Factory {}),
		Box::new(vector::vector_filter_features::Factory {}),
		Box::new(vector::vector_filter_layers::Factory {}),
		Box::new(vector::vector_filter_properties::Factory {}),
		Box::new(vector::vector_overzoom::Factory {}),
		Box::new(vector::vector_repair::Factory {}),
		Box::new(vector::vector_update_properties::Factory {}),
	]
}

pub fn read_operation_factories() -> Vec<Box<dyn ReadOperationFactoryTrait>> {
	vec![
		Box::new(read::from_color::Factory {}),
		Box::new(read::from_container::Factory {}),
		Box::new(read::from_csv::Factory {}),
		Box::new(read::from_debug::Factory {}),
		Box::new(read::from_geo::Factory {}),
		Box::new(read::from_grid::Factory {}),
		Box::new(read::from_h3::Factory {}),
		Box::new(read::from_merged_vector::Factory {}),
		Box::new(read::from_stacked_raster::Factory {}),
		Box::new(read::from_stacked::Factory {}),
		Box::new(read::from_tile::Factory {}),
		Box::new(read::from_tilejson::Factory {}),
		#[cfg(feature = "gdal")]
		Box::new(read::from_gdal::raster::Factory {}),
		#[cfg(feature = "gdal")]
		Box::new(read::from_gdal::dem::Factory {}),
	]
}

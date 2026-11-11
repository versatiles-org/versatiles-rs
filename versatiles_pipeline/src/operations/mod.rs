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
pub(crate) mod removed;
pub(crate) mod transform;
mod vector;

use crate::factory::{ReadOperationFactoryTrait, TileTransformFactoryTrait, TransformOperationFactoryTrait};

/// A transform operation, and the same operation as a per-tile transform if it is one.
struct Registered {
	operation: Box<dyn TransformOperationFactoryTrait>,
	per_tile: Option<Box<dyn TileTransformFactoryTrait>>,
}

fn plain(factory: impl TransformOperationFactoryTrait + 'static) -> Registered {
	Registered {
		operation: Box::new(factory),
		per_tile: None,
	}
}

fn per_tile<F>(factory: F) -> Registered
where
	F: TransformOperationFactoryTrait + TileTransformFactoryTrait + Clone + 'static,
{
	Registered {
		operation: Box::new(factory.clone()),
		per_tile: Some(Box::new(factory)),
	}
}

/// Every transform operation, each marked by whether it is a single per-tile transform.
///
/// One list rather than two, so that a per-tile operation cannot be registered as an
/// operation and forgotten as a transform, or the other way round.
fn transform_operations() -> Vec<Registered> {
	vec![
		plain(general::filter::Factory {}),
		plain(general::meta_update::Factory {}),
		plain(general::remap_coords::Factory {}),
		per_tile(dem::dem_colorize::Factory {}),
		plain(dem::dem_hillshade::Factory {}),
		plain(dem::dem_overscale::Factory {}),
		plain(dem::dem_overview::Factory {}),
		per_tile(dem::dem_quantize::Factory {}),
		plain(dem::dem_tile_resize::Factory {}),
		per_tile(raster::raster_flatten::Factory {}),
		per_tile(raster::raster_format::Factory {}),
		per_tile(raster::raster_levels::Factory {}),
		per_tile(raster::raster_mask::Factory {}),
		plain(raster::raster_overscale::Factory {}),
		plain(raster::raster_overview::Factory {}),
		plain(raster::raster_tile_resize::Factory {}),
		per_tile(vector::vector_filter_features::Factory {}),
		per_tile(vector::vector_filter_layers::Factory {}),
		per_tile(vector::vector_filter_properties::Factory {}),
		per_tile(vector::vector_mask::Factory {}),
		plain(vector::vector_overzoom::Factory {}),
		per_tile(vector::vector_reduce_to_style::Factory {}),
		per_tile(vector::vector_repair::Factory {}),
		per_tile(vector::vector_update_properties::Factory {}),
	]
}

pub fn transform_operation_factories() -> Vec<Box<dyn TransformOperationFactoryTrait>> {
	transform_operations().into_iter().map(|r| r.operation).collect()
}

/// The transform operations that can be built as a bare per-tile transform.
pub(crate) fn tile_transform_factories() -> Vec<Box<dyn TileTransformFactoryTrait>> {
	transform_operations().into_iter().filter_map(|r| r.per_tile).collect()
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

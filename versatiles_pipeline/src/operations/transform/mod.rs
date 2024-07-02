use crate::traits::TransformOperationFactoryTrait;

mod filter_bbox;
mod filter_zoom;
mod vectortiles_update_properties;

pub fn get_transform_operation_factories() -> Vec<Box<dyn TransformOperationFactoryTrait>> {
	vec![
		Box::new(filter_bbox::Factory {}),
		Box::new(filter_zoom::Factory {}),
		Box::new(vectortiles_update_properties::Factory {}),
	]
}

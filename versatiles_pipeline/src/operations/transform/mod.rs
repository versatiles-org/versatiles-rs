use crate::traits::TransformOperationFactoryTrait;

mod vectortiles_update_properties;

pub fn get_transform_operation_factories() -> Vec<Box<dyn TransformOperationFactoryTrait>> {
	vec![Box::new(vectortiles_update_properties::Factory {})]
}

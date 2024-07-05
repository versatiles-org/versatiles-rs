use crate::traits::ReadOperationFactoryTrait;

mod from_container;
pub mod from_debug;
mod from_overlayed;
mod from_vectortiles_merged;

pub fn get_read_operation_factories() -> Vec<Box<dyn ReadOperationFactoryTrait>> {
	vec![
		Box::new(from_container::Factory {}),
		Box::new(from_debug::Factory {}),
		Box::new(from_overlayed::Factory {}),
		Box::new(from_vectortiles_merged::Factory {}),
	]
}

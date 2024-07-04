use crate::traits::ReadOperationFactoryTrait;

mod from_container;
pub mod from_debug;
mod from_overlayed;

pub fn get_read_operation_factories() -> Vec<Box<dyn ReadOperationFactoryTrait>> {
	vec![
		Box::new(from_container::Factory {}),
		Box::new(from_debug::Factory {}),
		Box::new(from_overlayed::Factory {}),
	]
}

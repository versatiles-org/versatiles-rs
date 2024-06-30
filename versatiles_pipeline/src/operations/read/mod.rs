use crate::traits::ReadOperationFactoryTrait;

mod from_container;
mod from_dummy;
mod from_overlayed;

pub fn get_read_operation_factories() -> Vec<Box<dyn ReadOperationFactoryTrait>> {
	return vec![
		Box::new(from_dummy::Factory {}),
		Box::new(from_overlayed::Factory {}),
		Box::new(from_container::Factory {}),
	];
}

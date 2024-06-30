use crate::traits::ReadOperationFactoryTrait;

mod get_dummy_tiles;
mod get_overlayed;
mod get_tiles;

pub fn get_read_operation_factories() -> Vec<Box<dyn ReadOperationFactoryTrait>> {
	return vec![
		Box::new(get_dummy_tiles::Factory {}),
		Box::new(get_overlayed::Factory {}),
		Box::new(get_tiles::Factory {}),
	];
}

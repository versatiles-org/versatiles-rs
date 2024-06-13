mod read;
mod vectortiles_update_properties;

#[cfg(test)]
mod read_mock;

use super::{ReadableBuilder, ReadableBuilderTrait, TransformBuilder, TransformBuilderTrait};
use lazy_static::lazy_static;

lazy_static! {
	pub static ref READABLES: Vec<Box<dyn ReadableBuilderTrait>> = vec![
		ReadableBuilder::<read::Operation>::new(),
		#[cfg(test)]
		ReadableBuilder::<read_mock::Operation>::new(),
	];
	pub static ref TRANSFORMS: Vec<Box<dyn TransformBuilderTrait>> =
		vec![TransformBuilder::<vectortiles_update_properties::Operation>::new()];
}

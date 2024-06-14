mod read;
mod vectortiles_update_properties;

#[cfg(test)]
mod read_mock;

use super::{ReaderBuilder, ReaderBuilderTrait, TransformerBuilder, TransformerBuilderTrait};
use lazy_static::lazy_static;

lazy_static! {
	pub static ref READERS: Vec<Box<dyn ReaderBuilderTrait>> = vec![
		ReaderBuilder::<read::Operation>::new(),
		#[cfg(test)]
		ReaderBuilder::<read_mock::Operation>::new(),
	];
	pub static ref TRANSFORMERS: Vec<Box<dyn TransformerBuilderTrait>> =
		vec![TransformerBuilder::<vectortiles_update_properties::Operation>::new()];
}

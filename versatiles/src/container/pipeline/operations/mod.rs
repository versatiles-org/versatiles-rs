mod overlay_tiles;
mod read;
mod vectortiles_update_properties;

#[cfg(test)]
mod read_mock;

use super::{Builder, ComposerBuilderTrait, ReaderBuilderTrait, TransformerBuilderTrait};
use lazy_static::lazy_static;

lazy_static! {
	pub static ref READERS: Vec<Box<dyn ReaderBuilderTrait>> = vec![
		Builder::<read::Operation>::new(),
		#[cfg(test)]
		Builder::<read_mock::Operation>::new(),
	];
	pub static ref COMPOSERS: Vec<Box<dyn ComposerBuilderTrait>> =
		vec![Builder::<overlay_tiles::Operation>::new(),];
	pub static ref TRANSFORMERS: Vec<Box<dyn TransformerBuilderTrait>> =
		vec![Builder::<vectortiles_update_properties::Operation>::new()];
}

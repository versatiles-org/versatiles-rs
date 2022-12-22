#![allow(unused_variables)]

use std::path::PathBuf;

use super::{TileCompression, TileReader};

pub trait TileConverter {
	fn new(
		filename: &PathBuf,
		config: Option<TileConverterConfig>,
	) -> std::io::Result<Box<dyn TileConverter>>
	where
		Self: Sized,
	{
		panic!()
	}
	fn convert_from(&mut self, reader: Box<dyn TileReader>) -> std::io::Result<()> {
		panic!()
	}
}

pub struct TileConverterConfig {
	pub minimum_zoom: Option<u64>,
	pub maximum_zoom: Option<u64>,
	pub tile_compression: Option<TileCompression>,
}

impl TileConverterConfig {
	pub fn new_empty() -> Self {
		return TileConverterConfig {
			minimum_zoom: None,
			maximum_zoom: None,
			tile_compression: None,
		};
	}
	pub fn get_minimum_zoom(&self, other_minimum_zoom: u64) -> u64 {
		return self
			.minimum_zoom
			.unwrap_or(other_minimum_zoom)
			.max(other_minimum_zoom);
	}
	pub fn get_maximum_zoom(&self, other_maximum_zoom: u64) -> u64 {
		return self
			.maximum_zoom
			.unwrap_or(other_maximum_zoom)
			.min(other_maximum_zoom);
	}
}

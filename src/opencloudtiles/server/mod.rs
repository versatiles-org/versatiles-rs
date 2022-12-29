use std::collections::HashMap;

use super::containers::abstract_container::TileReader;

pub struct Server {
	port: u16,
	sources: HashMap<String, Box<dyn TileReader>>,
}

impl Server {
	pub fn new(port: u16) -> Server {
		return Server {
			port,
			sources: HashMap::new(),
		};
	}
	pub fn add_source(&mut self, name: &str, reader: Box<dyn TileReader>) {
		self.sources.insert(name.to_owned(), reader);
	}
	pub fn start(&mut self) {}
}

use crate::opencloudtiles::types::Precompression;
use enumset::EnumSet;
use hyper::{Body, Response, Result};

pub trait ServerSourceTrait: Send + Sync {
	fn get_name(&self) -> &str;
	fn get_data(&self, path: &[&str], accept: EnumSet<Precompression>) -> Result<Response<Body>>;
}

pub type ServerSourceBox = Box<dyn ServerSourceTrait>;

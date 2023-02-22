use crate::helper::Precompression;
use astra::Response;
use enumset::EnumSet;
use std::fmt::Debug;

pub trait ServerSourceTrait: Send + Sync + Debug {
	fn get_name(&self) -> &str;
	fn get_data(&self, path: &[&str], accept: EnumSet<Precompression>) -> Response;
}

pub type ServerSourceBox = Box<dyn ServerSourceTrait>;

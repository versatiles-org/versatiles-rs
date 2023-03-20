use crate::helper::Precompression;
use axum::response::Response;
use enumset::EnumSet;
use std::fmt::Debug;

pub trait ServerSourceTrait: Send + Sync + Debug {
	fn get_name(&self) -> String;
	fn get_info_as_json(&self) -> String;
	fn get_data(&self, path: &[&str], accept: EnumSet<Precompression>) -> Response;
}

pub type ServerSourceBox = Box<dyn ServerSourceTrait>;

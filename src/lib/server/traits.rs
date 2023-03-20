use crate::helper::Precompression;
use axum::{
	body::{Bytes, Full},
	response::Response,
};
use enumset::EnumSet;
use std::fmt::Debug;

pub trait ServerSourceTrait: Send + Sync + Debug {
	fn get_name(&self) -> String;
	fn get_info_as_json(&self) -> String;
	fn get_data(&self, path: &[&str], accept: EnumSet<Precompression>) -> Response<Full<Bytes>>;
}

pub type ServerSourceBox = Box<dyn ServerSourceTrait>;

use async_trait::async_trait;
use axum::{
	body::{Bytes, Full},
	response::Response,
};
use enumset::EnumSet;
use std::fmt::Debug;
use versatiles_shared::Precompression;

#[async_trait]
pub trait ServerSourceTrait: Send + Sync + Debug {
	fn get_name(&self) -> String;
	fn get_info_as_json(&self) -> String;
	async fn get_data(&self, path: &[&str], accept: EnumSet<Precompression>) -> Response<Full<Bytes>>;
}

pub type ServerSourceBox = Box<dyn ServerSourceTrait>;
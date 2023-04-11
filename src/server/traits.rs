use crate::shared::{Compression, Result};
use async_trait::async_trait;
use axum::{
	body::{Bytes, Full},
	response::Response,
};
use enumset::EnumSet;
use futures::lock::Mutex;
use std::{fmt::Debug, sync::Arc};

pub type ServerSource = Arc<Mutex<Box<dyn ServerSourceTrait>>>;

#[async_trait]
pub trait ServerSourceTrait: Send + Sync + Debug {
	fn get_name(&self) -> Result<String>;
	fn get_info_as_json(&self) -> Result<String>;
	async fn get_data(&mut self, path: &[&str], accept: EnumSet<Compression>) -> Response<Full<Bytes>>;
}

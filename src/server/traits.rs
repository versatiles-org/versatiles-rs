use crate::shared::{Blob, Compression, Result};
use async_trait::async_trait;
use enumset::EnumSet;
use futures::lock::Mutex;
use std::{fmt::Debug, option::Option, sync::Arc};

pub type ServerSource = Arc<Mutex<Box<dyn ServerSourceTrait>>>;

#[async_trait]
pub trait ServerSourceTrait: Send + Sync + Debug {
	fn get_name(&self) -> Result<String>;
	fn get_info_as_json(&self) -> Result<String>;
	async fn get_data(&mut self, path: &[&str], accept: EnumSet<Compression>) -> Option<ServerSourceResult>;
}

pub struct ServerSourceResult {
	pub blob: Blob,
	pub compression: Compression,
	pub mime: String,
}

pub fn make_result(blob: Blob, compression: &Compression, mime: &str) -> Option<ServerSourceResult> {
	Some(ServerSourceResult {
		blob,
		compression: compression.to_owned(),
		mime: mime.to_owned(),
	})
}

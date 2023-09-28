use anyhow::Result;
use async_trait::async_trait;
use std::{fmt::Debug, option::Option, sync::Arc};
use tokio::sync::Mutex;
use versatiles_lib::shared::{Blob, Compression, TargetCompression};

pub type ServerSource = Arc<Mutex<Box<dyn ServerSourceTrait>>>;

#[async_trait]
pub trait ServerSourceTrait: Send + Sync + Debug {
	fn get_name(&self) -> Result<String>;
	fn get_info_as_json(&self) -> Result<String>;
	async fn get_data(&mut self, path: &[&str], accept: &TargetCompression) -> Option<ServerSourceResult>;
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

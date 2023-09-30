use super::{response::SourceResponse, static_source_folder::Folder, static_source_tar::TarFile};
use anyhow::Result;
use async_trait::async_trait;
use std::{fmt::Debug, option::Option, sync::Arc};
use versatiles_lib::shared::TargetCompression;

#[async_trait]
pub trait StaticSourceTrait: Send + Sync + Debug {
	fn get_name(&self) -> Result<String>;
	async fn get_data(&self, path: &[&str], accept: &TargetCompression) -> Option<SourceResponse>;
}

#[derive(Clone)]
pub struct StaticSource {
	source: Arc<Box<dyn StaticSourceTrait>>,
}

impl StaticSource {
	pub fn new(filename: &str) -> Result<StaticSource> {
		Ok(StaticSource {
			source: Arc::new(if filename.ends_with(".tar") {
				Box::new(TarFile::from(filename)?)
			} else {
				Box::new(Folder::from(filename)?)
			}),
		})
	}
	pub async fn get_data(&self, path: &[&str], accept: &TargetCompression) -> Option<SourceResponse> {
		self.source.get_data(path, accept).await
	}
}

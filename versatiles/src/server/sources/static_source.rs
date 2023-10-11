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
	path: Vec<String>,
}

impl StaticSource {
	pub fn new(filename: &str, uncleaned_path: &str) -> Result<StaticSource> {
		let mut path: Vec<String> = uncleaned_path.trim().split('/').map(|s| s.to_string()).collect();
		while path.first() == Some(&String::from("")) {
			path.remove(0);
		}
		while path.last() == Some(&String::from("")) {
			path.pop();
		}

		Ok(StaticSource {
			source: Arc::new(if filename.ends_with(".tar") {
				Box::new(TarFile::from(filename)?)
			} else {
				Box::new(Folder::from(filename)?)
			}),
			path,
		})
	}
	pub async fn get_data(&self, path: &[&str], accept: &TargetCompression) -> Option<SourceResponse> {
		if self.path.is_empty() {
			self.source.get_data(path, accept).await
		} else {
			let mut path_vec: Vec<&str> = path.to_vec();
			for segment in self.path.iter() {
				if path_vec.is_empty() || (segment != path_vec[0]) {
					return None;
				}
				path_vec.remove(0);
			}
			self.source.get_data(path_vec.as_slice(), accept).await
		}
	}
}

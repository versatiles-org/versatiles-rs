use super::OperationTrait;
use crate::{
	container::pipeline::operations as op,
	utils::{parse_kdl, KDLNode},
};
use anyhow::{ensure, Result};
use std::path::{Path, PathBuf};

#[derive(versatiles_derive::KDLDecode, Clone, Debug)]
pub enum OperationKDLEnum {
	Read(op::ReadOperationKDL),
	OverlayTiles(op::OverlayTilesOperationKDL),
	VectortilesUpdateProperties(op::VectortilesUpdatePropertiesOperationKDL),
}

pub struct Factory {
	dir: PathBuf,
}

impl Factory {
	pub async fn operation_from_kdl(dir: &Path, text: &str) -> Result<Box<dyn OperationTrait>> {
		let mut kdl_nodes = parse_kdl(text)?;
		ensure!(
			kdl_nodes.len() == 1,
			"KDL must contain exactly one top node"
		);
		let kdl_node = kdl_nodes.pop().unwrap();

		let kdl_operation = OperationKDLEnum::from_kdl_node(&kdl_node)?;

		let factory = Factory {
			dir: dir.to_path_buf(),
		};

		factory.build_operation(kdl_operation).await
	}

	pub async fn build_operation(&self, node: OperationKDLEnum) -> Result<Box<dyn OperationTrait>> {
		use OperationKDLEnum::*;
		Ok(match node {
			Read(n) => Box::new(op::ReadOperation::new(n, self).await?),
			OverlayTiles(n) => Box::new(op::OverlayTilesOperation::new(n, self).await?),
			VectortilesUpdateProperties(n) => {
				Box::new(op::VectortilesUpdatePropertiesOperation::new(n, self).await?)
			}
		})
	}

	pub fn resolve_filename(&self, filename: &str) -> String {
		String::from(self.resolve_path(filename).to_str().unwrap())
	}

	pub fn resolve_path(&self, filename: &str) -> PathBuf {
		self.dir.join(filename)
	}

	pub fn get_docs() -> String {
		OperationKDLEnum::get_docs()
	}
}

use std::fmt::Debug;

use anyhow::{Result, bail};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TileSize {
	Size256,
	Size512,
}

impl TileSize {
	pub fn new(size: u16) -> Result<Self> {
		match size {
			256 => Ok(Self::Size256),
			512 => Ok(Self::Size512),
			_ => bail!("Invalid tile size: {}. Supported sizes are 256 or 512.", size),
		}
	}

	/// Returns the size of the tile in pixels.
	pub fn size(&self) -> u16 {
		match self {
			TileSize::Size256 => 256,
			TileSize::Size512 => 512,
		}
	}
}

impl Debug for TileSize {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "TileSize({})", self.size())
	}
}

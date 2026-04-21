use crate::TileCover;
use anyhow::{Result, bail};

pub trait TileCoverInfo {
	fn get_level(&self) -> u8;
	fn get_type_name(&self) -> &'static str;
	fn ensure_same_level(&self, other: &dyn TileCoverInfo, action: &str) -> Result<()> {
		if self.get_level() != other.get_level() {
			bail!(
				"Cannot {} {} with level={} with {} with level={}",
				action,
				self.get_type_name(),
				self.get_level(),
				other.get_type_name(),
				other.get_level()
			);
		}
		Ok(())
	}
}

impl TileCoverInfo for TileCover {
	fn get_level(&self) -> u8 {
		self.level()
	}

	fn get_type_name(&self) -> &'static str {
		"TileCover"
	}
}

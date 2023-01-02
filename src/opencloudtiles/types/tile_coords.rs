use std::fmt;

#[derive(Eq, PartialEq, Clone, Hash)]
pub struct TileCoord2 {
	pub x: u64,
	pub y: u64,
}
impl TileCoord2 {
	pub fn new(x: u64, y: u64) -> TileCoord2 {
		TileCoord2 { x, y }
	}
}

#[derive(Eq, PartialEq, Clone, Hash, Ord)]
pub struct TileCoord3 {
	pub z: u64,
	pub y: u64,
	pub x: u64,
}
impl TileCoord3 {
	pub fn new(z: u64, y: u64, x: u64) -> TileCoord3 {
		TileCoord3 { x, y, z }
	}
}

impl fmt::Debug for TileCoord3 {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!("TileCoord3[{}/{}/{}]", &self.z, &self.y, &self.x))
	}
}

impl PartialOrd for TileCoord3 {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		match self.z.partial_cmp(&other.z) {
			Some(core::cmp::Ordering::Equal) => {}
			ord => return ord,
		}
		match self.y.partial_cmp(&other.y) {
			Some(core::cmp::Ordering::Equal) => {}
			ord => return ord,
		}
		self.x.partial_cmp(&other.x)
	}
}

#[derive(Eq, PartialEq, Clone, Debug, Hash)]
pub struct TileCoord2 {
	pub x: u64,
	pub y: u64,
}
impl TileCoord2 {}

#[derive(Eq, PartialEq, Clone, Debug, Hash)]
pub struct TileCoord3 {
	pub x: u64,
	pub y: u64,
	pub z: u64,
}
impl TileCoord3 {}

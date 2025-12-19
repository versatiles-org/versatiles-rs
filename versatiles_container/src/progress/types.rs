#[derive(Debug, Clone)]
pub struct ProgressId(pub u32);

#[derive(Debug, Clone)]
pub struct ProgressState {
	pub id: ProgressId,
	pub message: String,
	pub position: u64,
	pub total: u64,
	pub start: std::time::Instant,
	pub next_draw: std::time::Instant,
	pub finished: bool,
}

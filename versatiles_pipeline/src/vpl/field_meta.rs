#[derive(Debug, Clone)]
pub struct VPLFieldMeta {
	pub name: String,
	pub rust_type: String,
	pub is_required: bool,
	pub is_sources: bool,
	pub doc: String,
}

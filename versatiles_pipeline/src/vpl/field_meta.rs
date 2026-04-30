#[derive(Debug, Clone)]
pub struct VPLFieldMeta {
	pub name: String,
	pub rust_type: String,
	pub is_required: bool,
	pub is_sources: bool,
	pub doc: String,
	/// Accepted string values for enum-typed fields (e.g.
	/// `["none", "gzip", "brotli", "zstd"]` for `Option<TileCompression>`).
	/// Empty for non-enum fields. Sourced from each enum's `variants()`
	/// method via the `VPLDecode` derive — single source of truth.
	pub enum_variants: Vec<&'static str>,
}

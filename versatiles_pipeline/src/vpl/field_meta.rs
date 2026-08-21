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
	///
	/// This is the *display* list: what a picker should offer, and what the
	/// reference and the TypeScript unions render. It is not the accepted set —
	/// see [`accepts`](Self::accepts).
	pub enum_variants: Vec<&'static str>,
	/// Whether the type's own parser accepts a given value.
	///
	/// `None` for fields with no enumerated type, which are decided by building.
	///
	/// The accepted set and the advertised set are two different things, and
	/// this is the accepted one: `TileFormat::variants` lists `mvt` and `jpg`,
	/// while the parser also takes `pbf` and `jpeg`. Offering both spellings in
	/// a picker would present one format as two, so `enum_variants` stays the
	/// canonical list — and validating against it would reject `format=pbf`,
	/// which builds. Asking the parser is the only way to get both right.
	///
	/// A function rather than a second, longer list, because aliases are the
	/// parser's business and a list to keep in step is how the mismatch started.
	/// The `VPLDecode` derive emits it from the same type as `enum_variants`, so
	/// the two cannot drift.
	pub accepts: Option<fn(&str) -> bool>,
}

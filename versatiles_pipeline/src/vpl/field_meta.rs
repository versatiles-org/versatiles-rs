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
	/// What the operation uses when this field is absent, as it would be written
	/// in VPL. `None` when there is no default.
	///
	/// A generated form otherwise shows an empty box for `from_color`'s `color`,
	/// which will use `000000`, and an identical empty box for `from_csv`'s
	/// `lon_column`, whose absence is an error. The information exists —
	/// `unwrap_or("000000")` is right there in the operation — and used to stop
	/// at the metadata boundary.
	///
	/// A string in VPL's own spelling, so a form can show it and a caller can
	/// put it in the document unchanged if the user wants it explicit. A typed
	/// value would have to be re-rendered by every consumer, each spelling
	/// `true` or `1.5` its own way.
	///
	/// `None` is not the same as required: an optional field with no default is
	/// one whose absence *does* something — `filter`'s `bbox` clips nothing at
	/// all when unset — and a form should say nothing rather than invent a
	/// value. Computed defaults ("the source's highest zoom level") are `None`
	/// too, since there is no literal to write.
	///
	/// Comes from `#[vpl(default = "…")]` on the field. `docs_style.rs` fails
	/// the build when that disagrees with the doc comment's "Defaults to `X`."
	/// sentence, or when a doc states a literal default and the attribute is
	/// missing — so the two are one fact stated twice, not two facts free to
	/// drift.
	pub default: Option<String>,
}

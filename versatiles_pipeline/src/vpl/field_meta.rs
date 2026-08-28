use versatiles_core::Bounds;

/// What an argument that names something in the *data* is naming.
///
/// The one thing on the argument surface that no type expresses. `lon_column`
/// names a column of the file `filename` names — a relationship between two
/// arguments, not a property of one — so a newtype has nowhere to put it, and a
/// consumer building a form is left matching on argument names and reading doc
/// comments (#260).
///
/// The `VPLDecode` derive checks that the argument named here exists on the
/// same operation, so unlike a doc phrase this cannot rot: rename `filename`
/// and the operation stops compiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldReference {
	/// Names a field — a CSV column, a GeoJSON property — of the data another
	/// argument points at. The argument is named here.
	FieldOf {
		/// The argument naming the data this field belongs to.
		argument: &'static str,
	},
	/// Names a field of the features arriving from upstream.
	FieldOfSource,
	/// Names a layer of the tiles arriving from upstream.
	LayerOfSource,
	/// Names a layer this operation creates.
	NewLayer,
	/// Names a field this operation writes into the features it creates.
	NewField,
}

/// Everything wrong with the values written for one parameter, or an empty
/// vector when they are fine.
///
/// A plain function pointer rather than a boxed closure: every one of these is
/// emitted by the `VPLDecode` derive and captures nothing, so the metadata stays
/// `Copy`-cheap to hand around.
///
/// See [`VPLFieldMeta::validate`] for what the messages look like and what they
/// can and cannot decide.
pub type ValueValidator = fn(&[String]) -> Vec<String>;

/// Everything the VPL reference generator knows about one operation argument.
///
/// Produced by the `VPLDecode` derive from the argument struct itself, so the
/// generated documentation cannot drift from the code that parses it.
#[derive(Debug, Clone)]
pub struct VPLFieldMeta {
	/// The argument's name as written in VPL.
	pub name: String,
	/// The Rust type the argument parses into, as a string.
	pub rust_type: String,
	/// Whether the operation fails when the argument is absent.
	pub is_required: bool,
	/// Whether this argument takes upstream sources rather than a value.
	pub is_sources: bool,
	/// The argument's doc comment, used to generate the VPL reference.
	pub doc: String,
	/// Accepted string values for enum-typed fields (e.g.
	/// `["none", "gzip", "brotli", "zstd"]` for `Option<TileCompression>`).
	/// Empty for non-enum fields. Sourced from each enum's `variants()`
	/// method via the `VPLDecode` derive — single source of truth.
	///
	/// This is the *display* list: what a picker should offer, and what the
	/// reference and the TypeScript unions render. It is not the accepted set —
	/// see [`validate`](Self::validate).
	pub enum_variants: Vec<&'static str>,
	/// What a numeric parameter accepts, for a form to render before anyone
	/// types. `None` for everything that is not a number.
	///
	/// The describing half of a range, and the counterpart of `enum_variants`:
	/// a closed set is described by its variants, a range by its bounds.
	/// [`validate`](Self::validate) is the *checking* half and cannot stand in
	/// for this — it is a predicate, so recovering a bound from it would mean
	/// probing it with candidate values, which is plainly not the intent.
	///
	/// Sourced from the field's own type where it has one (`ZoomLevel` answers
	/// `0..=30`), and from its Rust number type where it does not (`Option<u8>`
	/// answers `0..=255`). Both come from the same place the parser does, so
	/// what a form offers and what building accepts cannot drift (#260).
	pub bounds: Option<Bounds>,
	/// What this argument names in the data, when it names something there.
	///
	/// `None` for a value that stands on its own. See [`FieldReference`] for
	/// why this is metadata rather than a type: it is a relationship between
	/// two arguments, which no newtype can hold.
	pub refers_to: Option<FieldReference>,
	/// Everything wrong with the values written for this parameter, or an empty
	/// vector when they are fine.
	///
	/// `None` for parameters with nothing to check: a string list takes any
	/// number of values and judges none of them.
	///
	/// Each message is a verb phrase whose subject is the operation, so a caller
	/// puts its own name in front — `format!("'{}' {reason}", node.name)` yields
	/// `'from_debug' does not accept 'format=xyz'. Values: …`. Every problem is
	/// returned rather than the first, which is what lets an editor underline
	/// them all at once.
	///
	/// Two kinds of problem are decidable without building. *Shape* — how many
	/// values the accessor takes, and whether each parses as the field's numeric
	/// type — follows from the type mapping alone and is emitted for every
	/// field. *Value* is emitted for types that parse through `TryFrom<&str>`,
	/// and asks that parser rather than comparing against `enum_variants`: the
	/// accepted set and the advertised set are two different things, and this is
	/// the accepted one. `TileFormat::variants` lists `mvt` and `jpg` while the
	/// parser also takes `pbf` and `jpeg`, so validating against the list would
	/// reject `format=pbf`, which builds.
	///
	/// A function rather than a second, longer list, because aliases are the
	/// parser's business and a list to keep in step is how the mismatch started.
	/// The `VPLDecode` derive emits the same `parse::<T>()` and `T::try_from`
	/// calls the accessors on [`VPLNode`](super::VPLNode) make, so what this
	/// reports and what building enforces cannot drift.
	pub validate: Option<ValueValidator>,
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

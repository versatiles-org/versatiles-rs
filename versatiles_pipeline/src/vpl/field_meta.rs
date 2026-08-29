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

/// Which files an argument that names a file will take.
///
/// The third describing field, beside `enum_variants` for a closed set and
/// `bounds` for a range. `FilePath` says an argument names a file and stops
/// there, so a consumer offering a file dialog had a picker and no filter, and
/// the only per-field answer available was the doc comment's prose — "GeoJSON
/// polygon outside which pixels become transparent", "Path to the CSV or TSV
/// file", "Private key for this one `sftp://` source" (#260 typed the argument;
/// this says what it takes).
///
/// # Why three states and not a list
///
/// The fifteen arguments that name a file fall into three kinds, and one flat
/// list of extensions describes only the middle one — offered for all three it
/// is wrong twice:
///
/// * `from_geo` refuses an extension it does not know, so its list *is* the
///   accepted set and a dialog can filter on it.
/// * `from_csv` checks no extension at all. `.txt` builds today, so a filter
///   would block a pipeline that works.
/// * `from_gdal_raster` opens whatever GDAL was built with, and a private key
///   is conventionally extensionless — for both, the correct filter is no
///   filter, which is not the same fact as an absent one.
///
/// [`VPLFieldMeta::accepts`] is `Option<Accepts>` for that last distinction:
/// `None` means the argument does not name a file, [`Accepts::Any`] means it
/// names any file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accepts {
	/// Any file. The set is not ours to state — GDAL decides its own, and a
	/// private key has no conventional extension.
	Any,
	/// The extensions this argument is written with by convention. Anything
	/// else still builds, so a dialog preselects these and lets them be
	/// overridden.
	Suggested(&'static [&'static str]),
	/// Every extension that builds. Anything else is refused when the value is
	/// decoded, so a dialog can filter on this and be right.
	///
	/// Comes from the argument's own type, which is what refuses the others —
	/// see `GeoDataPath` and `TileFilePath`. Stated on a field instead it would
	/// be a second list to keep in step, which is the mismatch this exists to
	/// end.
	Only(&'static [&'static str]),
}

impl Accepts {
	/// The extensions to offer, or an empty slice for [`Any`](Self::Any).
	///
	/// For a dialog that wants the list and handles "no filter" by its
	/// emptiness. Callers that must tell [`Only`](Self::Only) from
	/// [`Suggested`](Self::Suggested) — a hard filter from a preselection —
	/// match on the variant instead.
	#[must_use]
	pub const fn extensions(&self) -> &'static [&'static str] {
		match self {
			Accepts::Any => &[],
			Accepts::Suggested(list) | Accepts::Only(list) => list,
		}
	}
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
	/// Which files this argument takes, when it names a file.
	///
	/// `None` when it does not name one. See [`Accepts`] for why "any file" is
	/// a state of its own rather than an empty list.
	pub accepts: Option<Accepts>,
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

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::{Attribute, DataStruct, DeriveInput, Field, Fields, Meta};

/// Metadata for mapping a Rust type to its VPL parsing method and documentation.
struct TypeMapping {
	/// The type pattern as a string (e.g., "String", "`Option<u8>`")
	pattern: &'static str,
	/// Human-readable type name for documentation
	display_name: &'static str,
	/// The method name on VPLNode to call for parsing this type
	method_name: &'static str,
	/// Whether this is a required field (affects documentation formatting)
	is_required: bool,
	/// Optional generic type parameter (e.g., "u8" for `property_number_option::<u8>`)
	generic_param: Option<&'static str>,
	/// Optional second generic parameter (e.g., "4" for array lengths)
	generic_param2: Option<&'static str>,
	/// True if `generic_param` names a string-enum exposing `fn variants() ->
	/// &'static [&'static str]`. The derive emits a call to that method into
	/// `VPLFieldMeta::enum_variants`, which downstream codegen
	/// (`versatiles_node`) uses to synthesize TS string-literal unions.
	is_enum: bool,
	/// How `generic_param`'s own parser takes the values, if it has one. The
	/// derive emits that parser as a probe into `VPLFieldMeta::validate`, so
	/// `check` can tell a bad value from a good one without building.
	///
	/// A superset of `is_enum`, and stated separately from it because the two
	/// answer different questions: `is_enum` is "can a picker offer a list",
	/// this is "can anything decide a value". `MaxTileBytes` takes a byte count
	/// or `none`, so it parses without having a list to offer.
	///
	/// Deliberately not inferred from `method_name`. Whether a value can be
	/// judged is a fact about the type, not about which accessor a field
	/// happens to route through — tying the two together is what left every
	/// `property_string_*` and `property_number_*` field unchecked (#257).
	parsed_from: ParsedFrom,
	/// True if `generic_param` names a type exposing `fn bounds() -> Bounds`.
	///
	/// The describing half of a range, and the counterpart of `is_enum`: a
	/// closed set is described by its `variants()`, a range by its `bounds()`.
	/// Emitted into `VPLFieldMeta::bounds` so a form can offer a spinner or a
	/// slider before anyone types, which `validate` — a predicate — cannot say
	/// (#260).
	///
	/// A field with no such type falls back to the range of its Rust number
	/// type, so `Option<u8>` still describes itself as `0..=255` whole numbers.
	has_bounds: bool,
}

/// How a type's own parser takes the values written for a field.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ParsedFrom {
	/// No parser of its own: only the shape of the values can be judged.
	Nothing,
	/// `TryFrom<&str>` — one value, judged on its own.
	Str,
	/// `TryFrom<&[String]>` — every value at once, so that a constraint
	/// *between* them is decidable. A bounding box whose west lies east of its
	/// east is only wrong as a group, and one probe per value cannot see it.
	Values,
	/// `crate::vpl::parse_bool` — the one case where the parser is a function
	/// rather than a type. `bool` is not ours to write a `TryFrom` impl for, and
	/// a newtype for a flag would cost every operation author more than the typo
	/// it catches.
	Bool,
}

/// All supported type mappings for VPLDecode.
const TYPE_MAPPINGS: &[TypeMapping] = &[
	// Required types
	TypeMapping {
		pattern: "String",
		display_name: "String",
		method_name: "property_string_required",
		is_required: true,
		generic_param: None,
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "bool",
		display_name: "Boolean",
		method_name: "property_bool_required",
		is_required: true,
		generic_param: None,
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Bool,
	},
	TypeMapping {
		pattern: "u8",
		display_name: "u8",
		method_name: "property_number_required",
		is_required: true,
		generic_param: Some("u8"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "u32",
		display_name: "u32",
		method_name: "property_number_required",
		is_required: true,
		generic_param: Some("u32"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "f64",
		display_name: "f64",
		method_name: "property_number_required",
		is_required: true,
		generic_param: Some("f64"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "[f64;4]",
		display_name: "[f64,f64,f64,f64]",
		method_name: "property_number_array_required",
		is_required: true,
		generic_param: Some("f64"),
		// The array accessors are generic over the length too, and it cannot be
		// inferred from the call, so a required array needs it spelled out just
		// as the optional ones do.
		generic_param2: Some("4"),
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "Vec<String>",
		display_name: "[String,...]",
		method_name: "property_string_list_required",
		is_required: true,
		generic_param: None,
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	// Optional types
	TypeMapping {
		pattern: "Option<bool>",
		display_name: "bool",
		method_name: "property_bool_option",
		is_required: false,
		generic_param: None,
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Bool,
	},
	TypeMapping {
		pattern: "Option<String>",
		display_name: "String",
		method_name: "property_string_option",
		is_required: false,
		generic_param: None,
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "Option<f32>",
		display_name: "f32",
		method_name: "property_number_option",
		is_required: false,
		generic_param: Some("f32"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "Option<u8>",
		display_name: "u8",
		method_name: "property_number_option",
		is_required: false,
		generic_param: Some("u8"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "Option<u16>",
		display_name: "u16",
		method_name: "property_number_option",
		is_required: false,
		generic_param: Some("u16"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "Option<u32>",
		display_name: "u32",
		method_name: "property_number_option",
		is_required: false,
		generic_param: Some("u32"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "Option<f64>",
		display_name: "f64",
		method_name: "property_number_option",
		is_required: false,
		generic_param: Some("f64"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "Option<[f64;3]>",
		display_name: "[f64,f64,f64]",
		method_name: "property_number_array_option",
		is_required: false,
		generic_param: Some("f64"),
		generic_param2: Some("3"),
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "Option<[f64;2]>",
		display_name: "[f64,f64]",
		method_name: "property_number_array_option",
		is_required: false,
		generic_param: Some("f64"),
		generic_param2: Some("2"),
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "Option<[f64;4]>",
		display_name: "[f64,f64,f64,f64]",
		method_name: "property_number_array_option",
		is_required: false,
		generic_param: Some("f64"),
		generic_param2: Some("4"),
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "Option<[u8;3]>",
		display_name: "[u8,u8,u8]",
		method_name: "property_number_array_option",
		is_required: false,
		generic_param: Some("u8"),
		generic_param2: Some("3"),
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "Option<Vec<String>>",
		display_name: "[String,...]",
		method_name: "property_string_list_option",
		is_required: false,
		generic_param: None,
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Nothing,
	},
	TypeMapping {
		pattern: "Option<TileCompression>",
		display_name: "TileCompression",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("TileCompression"),
		generic_param2: None,
		is_enum: true,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	// A closed set of two, so it carries `variants()` like the string enums: a
	// picker offers both and the TS bindings render `256 | 512`, instead of five
	// doc comments each restating the range — three of which used to (#260).
	TypeMapping {
		pattern: "TileSize",
		display_name: "TileSize",
		method_name: "property_enum_required",
		is_required: true,
		generic_param: Some("TileSize"),
		generic_param2: None,
		is_enum: true,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<TileSize>",
		display_name: "TileSize",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("TileSize"),
		generic_param2: None,
		is_enum: true,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	// Parsed but not enumerated, like `MaxTileBytes`: 31 levels is a range
	// rather than a list, so there is nothing for a picker to offer or for the
	// TS codegen to turn into a union. What the type buys is the range itself —
	// `u8` advertises 0..=255 for something that means 0..=30 (#260).
	// Shallower than the projection library on purpose — see `EpsgCode`. The
	// register's range is what a form can offer; whether a code is in the
	// register needs `proj.db`, and `check` performs no I/O.
	TypeMapping {
		pattern: "EpsgCode",
		display_name: "EPSG code",
		method_name: "property_enum_required",
		is_required: true,
		generic_param: Some("EpsgCode"),
		generic_param2: None,
		is_enum: false,
		has_bounds: true,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<EpsgCode>",
		display_name: "EPSG code",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("EpsgCode"),
		generic_param2: None,
		is_enum: false,
		has_bounds: true,
		parsed_from: ParsedFrom::Str,
	},
	// The bounded numbers, each declared with `bounded_number!` beside the
	// operation that takes it. Parsed but not enumerated, like `ZoomLevel`: a
	// range is not a list, and what they add is the range itself — three of
	// these were documented with a bound that nothing enforced (#260).
	// The two spellings of a GDAL placement. Written as one comma-separated
	// value, like `gdalinfo -json` prints them, so they parse from `&str`
	// rather than from a VPL list — the count and the ordering are the type's
	// business either way (#260).
	TypeMapping {
		pattern: "Option<CrsExtent>",
		display_name: "west,south,east,north",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("CrsExtent"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<RasterTransform>",
		display_name: "6 coefficients",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("RasterTransform"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<BandIndices>",
		display_name: "band indices",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("BandIndices"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<NodataValues>",
		display_name: "nodata values",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("NodataValues"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<Effort>",
		display_name: "0-100",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("Effort"),
		generic_param2: None,
		is_enum: false,
		has_bounds: true,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<Brightness>",
		display_name: "-255..255",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("Brightness"),
		generic_param2: None,
		is_enum: false,
		has_bounds: true,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<Contrast>",
		display_name: "above 0",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("Contrast"),
		generic_param2: None,
		is_enum: false,
		has_bounds: true,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<Gamma>",
		display_name: "above 0",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("Gamma"),
		generic_param2: None,
		is_enum: false,
		has_bounds: true,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<ZoomLevel>",
		display_name: "0-30",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("ZoomLevel"),
		generic_param2: None,
		is_enum: false,
		has_bounds: true,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<TileSchema>",
		display_name: "TileSchema",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("TileSchema"),
		generic_param2: None,
		is_enum: true,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<TileFormat>",
		display_name: "TileFormat",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("TileFormat"),
		generic_param2: None,
		is_enum: true,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<PointReductionStrategy>",
		display_name: "PointReductionStrategy",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("PointReductionStrategy"),
		generic_param2: None,
		is_enum: true,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	// Judged as a group rather than one value at a time: the count, the numbers
	// and the ordering between them are all decidable from the literal, and
	// only the whole list can show that west lies east of east (#255).
	TypeMapping {
		pattern: "GeoBBox",
		display_name: "[west,south,east,north]",
		method_name: "property_parsed_required",
		is_required: true,
		generic_param: Some("GeoBBox"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Values,
	},
	TypeMapping {
		pattern: "Option<GeoBBox>",
		display_name: "[west,south,east,north]",
		method_name: "property_parsed_option",
		is_required: false,
		generic_param: Some("GeoBBox"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Values,
	},
	// Judged as a group like `GeoBBox`: three numbers whose meanings differ from
	// each other, and a longitude that is not a longitude is only visible when
	// the parser sees which of the three it is looking at (#260).
	TypeMapping {
		pattern: "Option<GeoCenter>",
		display_name: "[lon,lat,zoom]",
		method_name: "property_parsed_option",
		is_required: false,
		generic_param: Some("GeoCenter"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Values,
	},
	TypeMapping {
		pattern: "Option<QualityByZoom>",
		display_name: "u8 | zoom:u8,...",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("QualityByZoom"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<CsvDelimiter>",
		display_name: "ASCII char",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("CsvDelimiter"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<SeparatorChar>",
		display_name: "char",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("SeparatorChar"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	// Like `MaxTileBytes`, parsed but not enumerated: every `RRGGBB` is a
	// colour, so there is no list for a picker to offer or for the TS codegen
	// to turn into a union. Judged as a group like `GeoBBox` rather than one
	// value at a time, because a colour has two spellings and they differ by
	// value count: `FF5733` is one value, `[255,87,51]` is three (#260).
	TypeMapping {
		pattern: "Option<HexColor>",
		display_name: "RRGGBB | [r,g,b]",
		method_name: "property_parsed_option",
		is_required: false,
		generic_param: Some("HexColor"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Values,
	},
	// Parsed via `TryFrom<&str>` like the string enums above, but its accepted
	// values aren't a closed set (`none` or any byte count), so `is_enum` stays
	// false — there's no variant list for the TS codegen to turn into a
	// string-literal union. `versatiles_node::codegen` special-cases it instead.
	TypeMapping {
		pattern: "Option<MaxTileBytes>",
		display_name: "u32 | none",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("MaxTileBytes"),
		generic_param2: None,
		is_enum: false,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<BlurFunction>",
		display_name: "BlurFunction",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("BlurFunction"),
		generic_param2: None,
		is_enum: true,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<DebugTileFormat>",
		display_name: "DebugTileFormat",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("DebugTileFormat"),
		generic_param2: None,
		is_enum: true,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<DemEncoding>",
		display_name: "DemEncoding",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("DemEncoding"),
		generic_param2: None,
		is_enum: true,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
	TypeMapping {
		pattern: "Option<RasterTileFormat>",
		display_name: "RasterTileFormat",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("RasterTileFormat"),
		generic_param2: None,
		is_enum: true,
		has_bounds: false,
		parsed_from: ParsedFrom::Str,
	},
];

/// What can be said about a field's *shape* — `(arity, number_type)` — from its
/// type mapping alone.
///
/// Both facts belong to the accessor rather than to this derive: `get_property`
/// errors unless a scalar parameter was given exactly one value, and
/// `property_number_array_option` unless an array was given exactly `N`. The
/// probe emitted from these calls the same `parse::<T>()`, so `check` reporting
/// a shape problem and the builder refusing to parse one are the same fact.
fn shape_of(mapping: &TypeMapping) -> (Option<usize>, Option<&'static str>) {
	if mapping.parsed_from == ParsedFrom::Values {
		// The type's parser owns the count and says something better about it
		// than "expects 4 values" — `GeoBBox` names west, south, east, north.
		return (None, None);
	}
	let method = mapping.method_name;
	if method.starts_with("property_number_array") {
		// `generic_param2` is the array length, as a literal to be parsed twice:
		// once here, once into the accessor's const generic.
		let n = mapping.generic_param2.and_then(|s| s.parse::<usize>().ok());
		(n, mapping.generic_param)
	} else if method.starts_with("property_string_list") {
		(None, None)
	} else if method.starts_with("property_number") {
		(Some(1), mapping.generic_param)
	} else {
		// String, bool and enum accessors all go through `get_property`, which
		// takes one value and no more. What each of them makes of that value is
		// `parsed_from`'s business, not this function's.
		(Some(1), None)
	}
}

/// Find a type mapping by its pattern string.
fn find_type_mapping(type_str: &str) -> Option<&'static TypeMapping> {
	TYPE_MAPPINGS.iter().find(|m| m.pattern == type_str)
}

/// Generate the list of supported types for error messages.
fn supported_types_list() -> String {
	TYPE_MAPPINGS.iter().map(|m| m.pattern).collect::<Vec<_>>().join(", ")
}

/// Reads `#[vpl(default = "…")]` off a field, if it carries one.
///
/// The value is VPL text rather than a Rust expression: it is what a form shows
/// and what a caller writes into the document to make the default explicit, so
/// `#[vpl(default = "000000")]` and `#[vpl(default = "false")]` are both plain
/// strings. Nothing here parses or type-checks it — `docs_style.rs` holds it
/// against the doc comment's "Defaults to `X`." sentence instead, which is the
/// copy a reader sees.
fn extract_vpl_default(attrs: &[Attribute]) -> Result<Option<String>, syn::Error> {
	let mut default = None;
	for attr in attrs {
		if !attr.path().is_ident("vpl") {
			continue;
		}
		attr.parse_nested_meta(|meta| {
			if meta.path.is_ident("default") {
				let value: syn::LitStr = meta.value()?.parse()?;
				default = Some(value.value());
				Ok(())
			} else {
				Err(meta.error("unknown VPLDecode field attribute; expected `default = \"…\"`"))
			}
		})?;
	}
	Ok(default)
}

/// Extract a doc comment from an attribute, if present.
pub fn extract_comment(attr: &Attribute) -> Option<String> {
	if attr.path().is_ident("doc")
		&& let Meta::NameValue(meta) = &attr.meta
		&& let syn::Expr::Lit(lit) = &meta.value
		&& let syn::Lit::Str(lit_str) = &lit.lit
	{
		let text = lit_str.value().trim().to_string();
		return if text.is_empty() { None } else { Some(text) };
	}
	None
}

/// Whether a line begins a Markdown block that owns its own line.
///
/// A table row, a heading, a blockquote and a horizontal rule each mean
/// something different from the line above them, so they must not be folded
/// into the preceding paragraph.
fn is_own_line_block(line: &str) -> bool {
	line.starts_with('|') || line.starts_with('#') || line.starts_with('>') || line == "---"
}

/// Whether a line starts a list item, whose wrapped continuation lines belong
/// to it rather than to a paragraph of their own.
fn starts_list_item(line: &str) -> bool {
	let rest = line
		.strip_prefix("- ")
		.or_else(|| line.strip_prefix("* "))
		.or_else(|| line.strip_prefix("+ "));
	if rest.is_some() {
		return true;
	}
	// An ordered item: digits, then `. ` or `) `.
	let digits = line.chars().take_while(char::is_ascii_digit).count();
	digits > 0 && matches!(line.get(digits..digits + 2), Some(". " | ") "))
}

/// Rewrap a struct's doc comment the way Markdown reads it.
///
/// A doc comment is hard-wrapped to fit the source file, but those line breaks
/// are not meaningful: Markdown treats a run of lines as one paragraph, and a
/// blank line as the break between two. Everything downstream — a tooltip, a
/// TypeScript doc comment, `versatiles help --raw` — wants the paragraph, not
/// the author's chosen column width, so each paragraph is joined back into one
/// line here.
///
/// Fenced code blocks are copied through untouched, because their line breaks
/// *are* meaningful. So are tables, headings and blockquotes. A list item keeps
/// its own line and absorbs its continuation lines, so a wrapped bullet becomes
/// one bullet rather than a bullet followed by a stray paragraph.
fn collapse_paragraphs(docs: &str) -> String {
	let mut out: Vec<String> = Vec::new();
	let mut paragraph: Vec<&str> = Vec::new();
	let mut in_fence = false;

	/// Emit whatever paragraph has been collected so far as one line.
	fn flush(paragraph: &mut Vec<&str>, out: &mut Vec<String>) {
		if !paragraph.is_empty() {
			out.push(paragraph.join(" "));
			paragraph.clear();
		}
	}

	for line in docs.lines() {
		let trimmed = line.trim();

		if trimmed.starts_with("```") {
			flush(&mut paragraph, &mut out);
			in_fence = !in_fence;
			out.push(trimmed.to_string());
		} else if in_fence {
			out.push(line.to_string());
		} else if trimmed.is_empty() || is_own_line_block(trimmed) {
			flush(&mut paragraph, &mut out);
			out.push(trimmed.to_string());
		} else if starts_list_item(trimmed) {
			// A new item ends the previous one, then collects its own
			// continuation lines through the branch below.
			flush(&mut paragraph, &mut out);
			paragraph.push(trimmed);
		} else {
			paragraph.push(trimmed);
		}
	}
	flush(&mut paragraph, &mut out);

	out.join("\n").trim().to_string()
}

/// Extract doc comments from struct attributes, preserving blank lines so the
/// rendered markdown keeps headings and code fences legible. `extract_comment`
/// collapses blank `///` lines to `None`; here we promote them to empty strings
/// so `join("\n")` yields real paragraph breaks, and
/// [`collapse_paragraphs`] then folds each paragraph back onto one line.
fn extract_struct_docs(attrs: &[Attribute]) -> String {
	let joined = attrs
		.iter()
		.filter_map(|a| {
			if a.path().is_ident("doc") {
				Some(extract_comment(a).unwrap_or_default())
			} else {
				None
			}
		})
		.collect::<Vec<String>>()
		.join("\n");
	collapse_paragraphs(&joined)
}

/// Splits a struct's rustdoc into its summary and the rest.
///
/// The summary is the first paragraph — everything up to the first blank line —
/// with its line breaks collapsed to spaces, so it renders as one sentence in a
/// tooltip or a picker. The remainder is returned verbatim.
///
/// Taking the first *paragraph* rather than the first *line* matters: rustdoc
/// summaries routinely wrap, and a line-based split silently truncates them
/// mid-sentence.
fn split_summary(docs: &str) -> (String, String) {
	let docs = docs.trim();
	let (summary, details) = match docs.split_once("\n\n") {
		Some((first, rest)) => (first, rest.trim()),
		None => (docs, ""),
	};

	let summary = summary
		.lines()
		.map(str::trim)
		.collect::<Vec<_>>()
		.join(" ")
		.trim()
		.to_string();

	(summary, details.to_string())
}

/// Metadata for a single field, used for code generation.
struct FieldMeta {
	name: String,
	rust_type: String,
	is_required: bool,
	is_sources: bool,
	doc: String,
	/// `Some(generic_param)` when the field is a string-enum (i.e. its
	/// `TypeMapping::is_enum` is `true`); `None` otherwise. The derive
	/// emits `<T>::variants().to_vec()` into the generated metadata for
	/// these fields, where `T` is this generic param.
	enum_type: Option<&'static str>,
	/// `Some(generic_param)` when the field is parsed through
	/// `TryFrom<&str>`; `None` otherwise. The derive emits a `<T>::try_from(v)`
	/// probe into `VPLFieldMeta::validate` for these.
	///
	/// A superset of `enum_type`: `MaxTileBytes` parses this way without having
	/// a closed variant list, and `check` can still tell a bad value from a good
	/// one even where a picker has nothing to offer.
	parsed_type: Option<&'static str>,
	/// How that parser takes the values — see [`ParsedFrom`].
	parsed_from: ParsedFrom,
	/// `Some(T)` when every value has to parse as the numeric type `T`; `None`
	/// when the accessor does not parse at all. The derive emits the same
	/// `parse::<T>()` the accessor makes.
	number_type: Option<&'static str>,
	/// How many values the accessor takes: `Some(1)` for a scalar, `Some(N)` for
	/// a fixed-size array, `None` for a list that takes any number.
	arity: Option<usize>,
	/// `Some(T)` when the field's own type describes its range through
	/// `fn bounds() -> Bounds`; `None` when it does not, in which case a scalar
	/// number falls back to the range of `number_type`.
	bounds_type: Option<&'static str>,
	/// The `#[vpl(default = "…")]` attribute, verbatim, or `None`.
	default: Option<String>,
}

/// Processed field information returned by `process_field`.
enum ProcessedField {
	/// A regular property field. `doc` is an expression evaluating to this
	/// parameter's rendered bullet line — a plain literal for most types, and a
	/// `format!` over the enum's `variants()` for enum-typed ones.
	Property { doc: TokenStream, parser: TokenStream },
	/// The special "sources" field with its doc string
	Sources { doc: String, parser: TokenStream },
}

/// Process a single struct field into parsing code.
fn process_field(field: &Field) -> Result<(String, ProcessedField, FieldMeta), syn::Error> {
	let field_name = &field.ident;
	let field_type = &field.ty;
	let field_str = field_name
		.as_ref()
		.ok_or_else(|| syn::Error::new_spanned(field, "field must have a name"))?
		.to_string();
	let field_type_str = quote!(#field_type).to_string().replace(' ', "");

	let raw_comment = field
		.attrs
		.iter()
		.filter_map(extract_comment)
		.collect::<Vec<String>>()
		.join(" ")
		.trim()
		.to_string();

	if field_str == "sources" {
		if field_type_str != "Vec<VPLPipeline>" {
			return Err(syn::Error::new_spanned(
				field_type,
				format!("type of 'sources' must be 'Vec<VPLPipeline>', but is '{field_type_str}'"),
			));
		}
		let doc = format!("### Sources\n\n{raw_comment}");
		let parser = quote! { sources: node.sources.clone() };
		let meta = FieldMeta {
			name: field_str.clone(),
			rust_type: field_type_str,
			is_required: true,
			is_sources: true,
			doc: raw_comment,
			enum_type: None,
			parsed_type: None,
			parsed_from: ParsedFrom::Nothing,
			// Sources are pipelines, not values; there is nothing to parse and
			// no count to hold them to. `check_sources` handles them instead.
			number_type: None,
			arity: None,
			bounds_type: None,
			default: None,
		};
		return Ok((field_str, ProcessedField::Sources { doc, parser }, meta));
	}

	let Some(mapping) = find_type_mapping(&field_type_str) else {
		return Err(syn::Error::new_spanned(
			field_type,
			format!(
				"unsupported type `{}` for VPLDecode.\nSupported types: {}",
				field_type_str,
				supported_types_list()
			),
		));
	};

	let method = format_ident!("{}", mapping.method_name);

	// Build the method call with appropriate generic parameters
	let call = match (mapping.generic_param, mapping.generic_param2) {
		(Some(g1), Some(g2)) => {
			let g1_ident = format_ident!("{}", g1);
			let g2_lit: proc_macro2::TokenStream = g2.parse().unwrap();
			quote! { node.#method::<#g1_ident, #g2_lit>(#field_str)? }
		}
		(Some(g1), None) => {
			let g1_ident = format_ident!("{}", g1);
			quote! { node.#method::<#g1_ident>(#field_str)? }
		}
		(None, _) => {
			quote! { node.#method(#field_str)? }
		}
	};

	let doc = field_doc_expr(&field_str, &raw_comment, mapping);
	let (arity, number_type) = shape_of(mapping);

	let default = extract_vpl_default(&field.attrs)?;
	if default.is_some() && mapping.is_required {
		return Err(syn::Error::new_spanned(
			field,
			format!("`{field_str}` is a required parameter, so it cannot have a default"),
		));
	}

	let meta = FieldMeta {
		name: field_str.clone(),
		rust_type: field_type_str,
		is_required: mapping.is_required,
		is_sources: false,
		doc: raw_comment,
		enum_type: if mapping.is_enum { mapping.generic_param } else { None },
		parsed_type: if mapping.parsed_from == ParsedFrom::Nothing {
			None
		} else {
			mapping.generic_param
		},
		parsed_from: mapping.parsed_from,
		number_type,
		arity,
		bounds_type: mapping.has_bounds.then_some(mapping.generic_param).flatten(),
		default,
	};

	Ok((
		field_str,
		ProcessedField::Property {
			doc,
			parser: quote! { #field_name: #call },
		},
		meta,
	))
}

/// Build the expression that renders one parameter's bullet line.
///
/// For most types this is a plain literal, fixed at expansion time. An
/// enum-typed field instead renders its accepted values from the enum's own
/// `variants()`, so that the reference and the parser cannot drift apart — and
/// since that list is only known at run time, `docs()` assembles its parameter
/// section from these expressions rather than from one baked-in literal.
fn field_doc_expr(field_str: &str, raw_comment: &str, mapping: &TypeMapping) -> TokenStream {
	let head = if mapping.is_required {
		format!("- **`{field_str}`: {} (required)**", mapping.display_name)
	} else {
		format!("- *`{field_str}`: {} (optional)*", mapping.display_name)
	};

	if let (true, Some(enum_type)) = (mapping.is_enum, mapping.generic_param) {
		let enum_ident = format_ident!("{}", enum_type);
		let tail = if raw_comment.is_empty() {
			String::new()
		} else {
			format!(" {raw_comment}")
		};
		return quote! {{
			let values = #enum_ident::variants()
				.iter()
				.map(|v| format!("`{v}`"))
				.collect::<Vec<String>>()
				.join(", ");
			format!("{} - Values: {}.{}", #head, values, #tail)
		}};
	}

	let literal = if raw_comment.is_empty() {
		head
	} else {
		format!("{head} - {raw_comment}")
	};
	quote! { #literal.to_string() }
}

/// The rendered documentation an operation's generated impl exposes.
struct Docs<'a> {
	/// Everything ahead of the generated parameter list, fixed at expansion time.
	prefix: &'a str,
	/// One expression per parameter, each evaluating to its bullet line.
	fields: &'a [TokenStream],
	/// First paragraph of the struct doc, collapsed to one line.
	summary: &'a str,
	/// Struct doc after the summary, plus the `sources` section.
	details: &'a str,
}

/// Build the probe that judges the *values* written for one field.
///
/// Which probe depends on what the field's type can decide for itself, never on
/// which accessor it routes through — that distinction is what `ParsedFrom`
/// carries, and conflating the two is the bug in #257.
fn value_check_expr(m: &FieldMeta) -> TokenStream {
	let fname = &m.name;
	// Ask the parser whether a value is accepted, rather than comparing
	// against `variants()`: the two are different sets on purpose, and
	// the parser is the one that decides whether a pipeline builds.
	//
	// What the message offers depends on what there is to offer. A closed
	// variant list is the useful thing to print, and it is `variants()`
	// rather than the accepted set: the aliases exist so that other
	// people's spellings work, not so that one format is listed under two
	// names. Where there is no list, the parser's own reason is the useful
	// thing — "expected 3, 4, 6, or 8 hex characters" tells a reader what
	// to write, and a bounding box can only explain itself.
	match (m.parsed_type, m.parsed_from) {
		(Some(parsed_ty), ParsedFrom::Values) => {
			let ty = format_ident!("{}", parsed_ty);
			// One probe for the whole list: the count and the relationship
			// between the values are exactly what this shape exists to see.
			quote! {
				if let Err(error) = <#ty as TryFrom<&[String]>>::try_from(values) {
					problems.push(format!(
						"does not accept '{}=[{}]': {:#}",
						#fname,
						values.join(","),
						error
					));
				}
			}
		}
		(Some(parsed_ty), ParsedFrom::Str) => {
			let ty = format_ident!("{}", parsed_ty);
			if let Some(enum_ty) = m.enum_type {
				let enum_ident = format_ident!("{}", enum_ty);
				quote! {
					for value in values {
						if <#ty as TryFrom<&str>>::try_from(value.as_str()).is_err() {
							problems.push(format!(
								"does not accept '{}={}'. Values: {}",
								#fname,
								value,
								#enum_ident::variants().join(", ")
							));
						}
					}
				}
			} else {
				quote! {
					for value in values {
						if let Err(error) = <#ty as TryFrom<&str>>::try_from(value.as_str()) {
							problems.push(format!("does not accept '{}={}': {:#}", #fname, value, error));
						}
					}
				}
			}
		}
		(_, ParsedFrom::Bool) => {
			// The same function the accessor calls, so a value `check` accepts
			// and a value that builds are the same set by construction.
			quote! {
				for value in values {
					if let Err(error) = crate::vpl::parse_bool(value) {
						problems.push(format!("does not accept '{}={}': {:#}", #fname, value, error));
					}
				}
			}
		}
		_ => {
			if let Some(number_ty) = m.number_type {
				let ty = format_ident!("{}", number_ty);
				// The parser's own error, rather than "is not a number": it
				// is the one that knows 300 is a fine number and a bad `u8`.
				quote! {
					for value in values {
						if let Err(error) = value.parse::<#ty>() {
							problems.push(format!("does not accept '{}={}': {}", #fname, value, error));
						}
					}
				}
			} else {
				quote! {}
			}
		}
	}
}

/// Build the `VPLFieldMeta` literal the derive emits for one field.
///
/// Everything in it is fixed at expansion time except the two run-time calls
/// into the field's own type — `variants()` for the display list, and the
/// parser probes inside `validate` — which is what keeps the metadata from
/// drifting away from the code that parses.
fn field_meta_expr(m: &FieldMeta) -> TokenStream {
	let fname = &m.name;
	let rtype = &m.rust_type;
	let required = m.is_required;
	let is_sources = m.is_sources;
	let fdoc = &m.doc;
	// For enum-typed fields, ask the enum itself for its accepted
	// variants — single source of truth, kept in sync with the
	// `TryFrom<&str>` impl by the enum's own round-trip test.
	let variants_expr: TokenStream = if let Some(enum_ty) = m.enum_type {
		let ty = format_ident!("{}", enum_ty);
		quote! { #ty::variants().to_vec() }
	} else {
		quote! { Vec::new() }
	};
	// The describing half of a range, beside `variants()` above. A type that
	// owns its range answers for itself; a plain number falls back to what its
	// Rust type accepts, which is still worth saying — `Option<u8>` is whole
	// numbers from 0 to 255, and a form can step it by one.
	let bounds_expr: TokenStream = match (m.bounds_type, m.arity, m.number_type) {
		(Some(bounds_ty), _, _) => {
			let ty = format_ident!("{}", bounds_ty);
			quote! { Some(#ty::bounds()) }
		}
		(None, Some(1), Some(number_ty)) => {
			let ty = format_ident!("{}", number_ty);
			quote! { Some(<#ty as crate::vpl::NumberBounds>::BOUNDS) }
		}
		// An array's bound would be per element, and nothing consumes that yet.
		_ => quote! { None },
	};
	let value_check = value_check_expr(m);
	let arity_check: TokenStream = match m.arity {
		Some(1) => quote! {
			if values.len() != 1 {
				problems.push(format!("expects a single value for '{}', got {}", #fname, values.len()));
			}
		},
		Some(n) => {
			let n_lit = proc_macro2::Literal::usize_unsuffixed(n);
			quote! {
				if values.len() != #n_lit {
					problems.push(format!(
						"expects {} values for '{}', got {}",
						#n_lit,
						#fname,
						values.len()
					));
				}
			}
		}
		None => quote! {},
	};
	// A parameter with nothing to say about either its values or how
	// many of them there are gets `None`, so `check` can skip it.
	let validate_expr: TokenStream = if value_check.is_empty() && arity_check.is_empty() {
		quote! { None }
	} else {
		// Values first: which value is wrong is more specific than how
		// many there are, and a reader wants the specific one first.
		quote! {
			Some(|values: &[String]| -> Vec<String> {
				let mut problems: Vec<String> = Vec::new();
				#value_check
				#arity_check
				problems
			})
		}
	};
	let default_expr: TokenStream = if let Some(default) = &m.default {
		quote! { Some(#default.to_string()) }
	} else {
		quote! { None }
	};
	quote! {
		crate::vpl::VPLFieldMeta {
			name: #fname.to_string(),
			rust_type: #rtype.to_string(),
			is_required: #required,
			is_sources: #is_sources,
			doc: #fdoc.to_string(),
			enum_variants: #variants_expr,
			bounds: #bounds_expr,
			validate: #validate_expr,
			default: #default_expr,
		}
	}
}

fn build_impl_tokens(
	name: &Ident,
	field_names: &[String],
	parser_fields: &[TokenStream],
	docs: &Docs<'_>,
	field_metas: &[FieldMeta],
) -> TokenStream {
	let Docs {
		prefix: doc_prefix,
		fields: doc_fields,
		summary: doc_summary,
		details: doc_details,
	} = *docs;
	let meta_entries: Vec<TokenStream> = field_metas.iter().map(field_meta_expr).collect();

	quote! {
		impl #name {
			pub fn from_vpl_node(node: &VPLNode) -> Result<Self> {
				// scan node.property_names to ensure, that all properties are also defined in field_names
				let argument_names: Vec<String> = vec![#(#field_names.to_string()),*];
				let property_names = node.property_names();
				for property_name in property_names {
					if !argument_names.contains(&property_name) {
						anyhow::bail!(
							"The '{}' operation does not have a parameter '{}'.\nSupported parameters: '{}'",
							node.name,
							property_name,
							argument_names.join("', '")
						);
					}
				}

				Ok(Self {
					#(#parser_fields),*
				})
			}

			pub fn docs() -> String {
				let parameters: Vec<String> = vec![#(#doc_fields),*];
				let mut parts: Vec<String> = Vec::new();
				if !#doc_prefix.is_empty() {
					parts.push(#doc_prefix.to_string());
				}
				if !parameters.is_empty() {
					parts.push(format!("### Parameters\n\n{}", parameters.join("\n")));
				}
				parts.join("\n\n").trim().to_string()
			}

			/// The first paragraph of the operation's documentation — one
			/// sentence, suitable for a tooltip or a picker entry.
			#[cfg(feature = "codegen")]
			pub fn doc_summary() -> String {
				#doc_summary.to_string()
			}

			/// Everything after the summary, excluding the generated parameter
			/// list (which is already available, structured, via
			/// `field_metadata`). Empty when there is nothing more to say.
			#[cfg(feature = "codegen")]
			pub fn doc_details() -> String {
				#doc_details.to_string()
			}

			pub fn field_metadata() -> Vec<crate::vpl::VPLFieldMeta> {
				vec![#(#meta_entries),*]
			}
		}
	}
}

/// Decode a struct definition into VPL parsing code.
///
/// Returns `Ok(TokenStream)` on success, or `Err(syn::Error)` if the struct
/// contains unsupported field types or is not a named-field struct.
pub fn decode_struct(input: DeriveInput, data_struct: DataStruct) -> Result<TokenStream, syn::Error> {
	let name = input.ident;
	let doc_struct = extract_struct_docs(&input.attrs);

	let fields = match data_struct.fields {
		Fields::Named(fields_named) => fields_named.named,
		_ => {
			return Err(syn::Error::new_spanned(
				&name,
				"VPLDecode can only be derived for structs with named fields",
			));
		}
	};

	let mut parser_fields: Vec<TokenStream> = Vec::new();
	let mut doc_fields: Vec<TokenStream> = Vec::new();
	let mut doc_sources: Option<String> = None;
	let mut field_names: Vec<String> = Vec::new();
	let mut field_metas: Vec<FieldMeta> = Vec::new();

	for field in fields {
		let (field_str, processed, meta) = process_field(&field)?;

		if field_str == "sources" && doc_sources.is_some() {
			return Err(syn::Error::new_spanned(
				&field.ident,
				"'sources' field is already defined",
			));
		}

		field_names.push(field_str);
		field_metas.push(meta);
		match processed {
			ProcessedField::Sources { doc, parser } => {
				doc_sources = Some(doc);
				parser_fields.push(parser);
			}
			ProcessedField::Property { doc, parser } => {
				doc_fields.push(doc);
				parser_fields.push(parser);
			}
		}
	}

	// Everything ahead of the generated parameter list. `docs()` appends the
	// parameters at run time, because enum-typed ones interpolate their
	// `variants()`; the prose ahead of them is fixed at expansion time.
	let doc_sources = doc_sources.unwrap_or_default();
	let doc_prefix = vec![doc_struct.clone(), doc_sources.clone()]
		.into_iter()
		.filter(|s| !s.is_empty())
		.collect::<Vec<String>>()
		.join("\n\n")
		.trim()
		.to_string();

	// `summary` and `details` are the same prose without the generated
	// parameter section, so a UI never has to split markdown to find out what an
	// operation does.
	let (doc_summary, doc_struct_rest) = split_summary(&doc_struct);
	let doc_details = vec![doc_struct_rest, doc_sources]
		.into_iter()
		.filter(|s| !s.is_empty())
		.collect::<Vec<String>>()
		.join("\n\n")
		.trim()
		.to_string();

	Ok(build_impl_tokens(
		&name,
		&field_names,
		&parser_fields,
		&Docs {
			prefix: &doc_prefix,
			fields: &doc_fields,
			summary: &doc_summary,
			details: &doc_details,
		},
		&field_metas,
	))
}

#[cfg(test)]
mod tests {
	use syn::{DeriveInput, parse_quote};

	use super::{ParsedFrom, TYPE_MAPPINGS, decode_struct, shape_of};

	/// The table is hand-maintained, and the two flags on it are the only place
	/// where a new type can be added without the derive noticing that nothing
	/// checks its values. These are the invariants a reviewer would otherwise
	/// have to hold in their head.
	#[test]
	fn the_type_mapping_table_is_self_consistent() {
		for m in TYPE_MAPPINGS {
			assert!(
				!m.is_enum || m.parsed_from == ParsedFrom::Str,
				"{}: a type with `variants()` parses from `&str` too",
				m.pattern
			);
			assert_eq!(
				matches!(m.parsed_from, ParsedFrom::Str | ParsedFrom::Values),
				m.generic_param.is_some() && !m.method_name.starts_with("property_number"),
				"{}: a type that parses for itself is the one named in `generic_param`",
				m.pattern
			);
			assert_eq!(
				m.parsed_from == ParsedFrom::Bool,
				m.method_name.starts_with("property_bool"),
				"{}: `ParsedFrom::Bool` and the bool accessor go together",
				m.pattern
			);
			// A field checked by its parser is checked by that and nothing else:
			// a numeric probe on the same values would be a second opinion.
			let (arity, number_type) = shape_of(m);
			assert!(
				m.parsed_from == ParsedFrom::Nothing || number_type.is_none(),
				"{}: parses through both its own type and `parse::<T>()`",
				m.pattern
			);
			// The probe and the accessor have to agree about how many values
			// they take, or `check` would judge a list the builder never sees
			// as one — and vice versa.
			assert_eq!(
				m.parsed_from == ParsedFrom::Values,
				m.method_name.starts_with("property_parsed"),
				"{}: `ParsedFrom::Values` and the list accessor go together",
				m.pattern
			);
			assert!(
				m.parsed_from != ParsedFrom::Values || arity.is_none(),
				"{}: a group-parsed type owns its own count",
				m.pattern
			);
		}
	}

	fn pretty_tokens(ts: &proc_macro2::TokenStream) -> Vec<String> {
		prettyplease::unparse(&syn::parse_file(&ts.to_string()).unwrap())
			.split('\n')
			.map(std::string::ToString::to_string)
			.collect()
	}

	#[test]
	fn test_decode_struct_simple() {
		// Simple struct with one String field
		let input: DeriveInput = parse_quote!(
			/// Struct documentation
			struct Test {
				#[doc = "Field documentation"]
				field1: String,
			}
		);
		let data_struct = match &input.data {
			syn::Data::Struct(ds) => ds.clone(),
			_ => panic!("Expected struct data"),
		};
		let ts = decode_struct(input.clone(), data_struct).unwrap();
		let code = pretty_tokens(&ts).join("\n");
		assert!(code.contains("pub fn from_vpl_node(node: &VPLNode) -> Result<Self>"));
		assert!(code.contains("field1: node.property_string_required(\"field1\")?"));
		assert!(code.contains("pub fn docs() -> String"));
		assert!(code.contains("Struct documentation"));
		assert!(code.contains("Field documentation"));
		assert!(code.contains("pub fn field_metadata()"));
		assert!(code.contains("\"field1\".to_string()"));
		assert!(code.contains("\"String\""));
		let code_no_spaces = code.replace(' ', "");
		assert!(code_no_spaces.contains("is_required:true"));
		assert!(code_no_spaces.contains("is_sources:false"));
	}

	/// Helper to verify decode_struct output for a single-field struct.
	fn assert_field_type_decodes(input: DeriveInput, getter: &str, comment: &str) {
		let data_struct = match &input.data {
			syn::Data::Struct(ds) => ds.clone(),
			_ => panic!("Expected struct data"),
		};
		let ts = decode_struct(input, data_struct).unwrap();
		let code = pretty_tokens(&ts).join("\n");
		assert!(
			code.contains(&format!("v: node.{getter}(\"v\")?")),
			"missing getter {getter}"
		);
		assert!(code.contains(comment), "missing comment {comment}");
		assert!(code.contains("pub fn field_metadata() -> Vec<crate::vpl::VPLFieldMeta>"));
	}

	#[test]
	fn test_decode_struct_required_types() {
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: String,
				}
			),
			"property_string_required",
			"**`v`: String (required)**",
		);
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: bool,
				}
			),
			"property_bool_required",
			"**`v`: Boolean (required)**",
		);
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: u8,
				}
			),
			"property_number_required::<u8>",
			"**`v`: u8 (required)**",
		);
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: [f64; 4],
				}
			),
			"property_number_array_required::<f64, 4>",
			"**`v`: [f64,f64,f64,f64] (required)**",
		);
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: Vec<String>,
				}
			),
			"property_string_list_required",
			"**`v`: [String,...] (required)**",
		);
	}

	#[test]
	fn test_decode_struct_optional_types() {
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: Option<bool>,
				}
			),
			"property_bool_option",
			"*`v`: bool (optional)*",
		);
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: Option<String>,
				}
			),
			"property_string_option",
			"*`v`: String (optional)*",
		);
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: Option<f32>,
				}
			),
			"property_number_option::<f32>",
			"*`v`: f32 (optional)*",
		);
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: Option<u8>,
				}
			),
			"property_number_option::<u8>",
			"*`v`: u8 (optional)*",
		);
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: Option<u32>,
				}
			),
			"property_number_option::<u32>",
			"*`v`: u32 (optional)*",
		);
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: Option<f64>,
				}
			),
			"property_number_option::<f64>",
			"*`v`: f64 (optional)*",
		);
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: Option<[f64; 4]>,
				}
			),
			"property_number_array_option::<f64, 4>",
			"*`v`: [f64,f64,f64,f64] (optional)*",
		);
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: Option<TileFormat>,
				}
			),
			"property_enum_option::<TileFormat>",
			"*`v`: TileFormat (optional)*",
		);
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: Option<Vec<String>>,
				}
			),
			"property_string_list_option",
			"*`v`: [String,...] (optional)*",
		);
		assert_field_type_decodes(
			parse_quote!(
				struct T {
					v: Option<MaxTileBytes>,
				}
			),
			"property_enum_option::<MaxTileBytes>",
			"*`v`: u32 | none (optional)*",
		);
	}

	#[test]
	fn test_decode_struct_with_sources() {
		// Struct with sources field
		let input: DeriveInput = parse_quote!(
			/// Top-level doc
			struct Pipeline {
				#[doc = "List of sources"]
				sources: Vec<VPLPipeline>,
			}
		);
		let data_struct = match &input.data {
			syn::Data::Struct(ds) => ds.clone(),
			_ => panic!("Expected struct data"),
		};
		let ts = decode_struct(input.clone(), data_struct).unwrap();
		let code = ts.to_string();
		// Ensure docs includes Sources section
		assert!(code.contains("### Sources"));
		assert!(code.contains("List of sources"));
	}

	#[test]
	fn test_decode_struct_unsupported_type_error() {
		// Struct with unsupported type should return error
		let input: DeriveInput = parse_quote!(
			struct T {
				v: i128,
			}
		);
		let data_struct = match &input.data {
			syn::Data::Struct(ds) => ds.clone(),
			_ => panic!("Expected struct data"),
		};
		let result = decode_struct(input, data_struct);
		assert!(result.is_err());
		let err = result.unwrap_err();
		let msg = err.to_string();
		assert!(msg.contains("unsupported type"));
		assert!(msg.contains("i128"));
	}

	#[test]
	fn test_field_metadata_generated() {
		let input: DeriveInput = parse_quote!(
			/// MyStruct docs
			struct MyStruct {
				/// Required field_name
				field_name: String,
				/// Optional level
				level: Option<u8>,
				/// Sources list
				sources: Vec<VPLPipeline>,
			}
		);
		let data_struct = match &input.data {
			syn::Data::Struct(ds) => ds.clone(),
			_ => panic!("Expected struct data"),
		};
		let ts = decode_struct(input, data_struct).unwrap();
		let code = pretty_tokens(&ts).join("\n");

		// Verify field_metadata method is generated
		assert!(code.contains("pub fn field_metadata()"));

		// Verify field entries exist (prettyplease uses "key : value" formatting)
		assert!(code.contains("\"field_name\".to_string()"));
		assert!(code.contains("\"String\""));

		assert!(code.contains("\"level\".to_string()"));
		assert!(code.contains("\"Option<u8>\""));

		assert!(code.contains("\"sources\".to_string()"));
		assert!(code.contains("\"Vec<VPLPipeline>\""));
		let code_no_spaces = code.replace(' ', "");
		assert!(code_no_spaces.contains("is_sources:true"));
	}

	#[test]
	fn collapse_paragraphs_folds_wrapped_prose_onto_one_line() {
		let input = "Reads a container.\n\nThe format is detected\nfrom the file's\nextension.";
		assert_eq!(
			super::collapse_paragraphs(input),
			"Reads a container.\n\nThe format is detected from the file's extension."
		);
	}

	#[test]
	fn collapse_paragraphs_keeps_fenced_blocks_verbatim() {
		let input = "Intro line\nwrapped.\n\n```vpl\nfrom_container filename=\"a.versatiles\"\nfrom_debug format=png\n```\n\nTrailing\nprose.";
		assert_eq!(
			super::collapse_paragraphs(input),
			"Intro line wrapped.\n\n```vpl\nfrom_container filename=\"a.versatiles\"\nfrom_debug format=png\n```\n\nTrailing prose."
		);
	}

	#[test]
	fn collapse_paragraphs_keeps_tables_and_headings_on_their_own_lines() {
		let input = "### Formats\n\n| Ext | Format |\n| --- | ------ |\n| shp | Esri   |";
		assert_eq!(super::collapse_paragraphs(input), input);
	}

	#[test]
	fn collapse_paragraphs_folds_a_wrapped_list_item_into_one_item() {
		let input = "- first item that\n  wraps over lines\n- second item\n\nAfter.";
		assert_eq!(
			super::collapse_paragraphs(input),
			"- first item that wraps over lines\n- second item\n\nAfter."
		);
	}

	#[test]
	fn collapse_paragraphs_recognises_ordered_items() {
		assert!(super::starts_list_item("1. one"));
		assert!(super::starts_list_item("12) twelve"));
		assert!(!super::starts_list_item("1.5 is not a list"));
		assert!(!super::starts_list_item("2^z - 1 - x"));
	}

	#[test]
	fn split_summary_takes_the_whole_first_paragraph() {
		// A wrapped summary must survive intact — splitting on the first *line*
		// is what truncated four operations' JSDoc mid-sentence.
		let (summary, details) = super::split_summary(
			"Merges multiple vector tile sources.\nEach resulting tile will contain all the features.\n\n### Sources\n\nAll sources must provide vector tiles.",
		);
		assert_eq!(
			summary,
			"Merges multiple vector tile sources. Each resulting tile will contain all the features."
		);
		assert_eq!(details, "### Sources\n\nAll sources must provide vector tiles.");
	}

	#[test]
	fn split_summary_handles_a_lone_paragraph() {
		let (summary, details) = super::split_summary("Reads a tile container.");
		assert_eq!(summary, "Reads a tile container.");
		assert!(details.is_empty(), "nothing to say beyond the summary");
	}

	#[test]
	fn split_summary_handles_empty_docs() {
		let (summary, details) = super::split_summary("");
		assert!(summary.is_empty());
		assert!(details.is_empty());
	}

	#[test]
	fn generated_code_exposes_summary_and_details() {
		let input: DeriveInput = parse_quote!(
			/// Does a thing.
			/// Continues the summary on a second line.
			///
			/// A longer explanation.
			struct Test {
				#[doc = "Field documentation"]
				field1: String,
			}
		);
		let data_struct = match &input.data {
			syn::Data::Struct(ds) => ds.clone(),
			_ => panic!("Expected struct data"),
		};
		let code = pretty_tokens(&decode_struct(input, data_struct).unwrap()).join("\n");

		assert!(code.contains("pub fn doc_summary() -> String"));
		assert!(code.contains("pub fn doc_details() -> String"));
		// The summary is the joined first paragraph, and carries no parameter list.
		assert!(code.contains("Does a thing. Continues the summary on a second line."));
		// `docs()` is unchanged and still carries the generated parameter list,
		// because that is what the operation reference renders.
		assert!(code.contains("### Parameters"));
	}
}

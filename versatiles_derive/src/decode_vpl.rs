use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::{Attribute, DataStruct, DeriveInput, Field, Fields, Meta};

/// Metadata for mapping a Rust type to its VPL parsing method and documentation.
struct TypeMapping {
	/// The type pattern as a string (e.g., "String", "Option<u8>")
	pattern: &'static str,
	/// Human-readable type name for documentation
	display_name: &'static str,
	/// The method name on VPLNode to call for parsing this type
	method_name: &'static str,
	/// Whether this is a required field (affects documentation formatting)
	is_required: bool,
	/// Optional generic type parameter (e.g., "u8" for property_number_option::<u8>)
	generic_param: Option<&'static str>,
	/// Optional second generic parameter (e.g., "4" for array lengths)
	generic_param2: Option<&'static str>,
	/// True if `generic_param` names a string-enum exposing `fn variants() ->
	/// &'static [&'static str]`. The derive emits a call to that method into
	/// `VPLFieldMeta::enum_variants`, which downstream codegen
	/// (`versatiles_node`) uses to synthesize TS string-literal unions.
	is_enum: bool,
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
	},
	TypeMapping {
		pattern: "bool",
		display_name: "Boolean",
		method_name: "property_bool_required",
		is_required: true,
		generic_param: None,
		generic_param2: None,
		is_enum: false,
	},
	TypeMapping {
		pattern: "u8",
		display_name: "u8",
		method_name: "property_number_required",
		is_required: true,
		generic_param: Some("u8"),
		generic_param2: None,
		is_enum: false,
	},
	TypeMapping {
		pattern: "[f64;4]",
		display_name: "[f64,f64,f64,f64]",
		method_name: "property_number_array_required",
		is_required: true,
		generic_param: Some("f64"),
		generic_param2: None,
		is_enum: false,
	},
	TypeMapping {
		pattern: "Vec<String>",
		display_name: "[String,...]",
		method_name: "property_string_list_required",
		is_required: true,
		generic_param: None,
		generic_param2: None,
		is_enum: false,
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
	},
	TypeMapping {
		pattern: "Option<String>",
		display_name: "String",
		method_name: "property_string_option",
		is_required: false,
		generic_param: None,
		generic_param2: None,
		is_enum: false,
	},
	TypeMapping {
		pattern: "Option<f32>",
		display_name: "f32",
		method_name: "property_number_option",
		is_required: false,
		generic_param: Some("f32"),
		generic_param2: None,
		is_enum: false,
	},
	TypeMapping {
		pattern: "Option<u8>",
		display_name: "u8",
		method_name: "property_number_option",
		is_required: false,
		generic_param: Some("u8"),
		generic_param2: None,
		is_enum: false,
	},
	TypeMapping {
		pattern: "Option<u16>",
		display_name: "u16",
		method_name: "property_number_option",
		is_required: false,
		generic_param: Some("u16"),
		generic_param2: None,
		is_enum: false,
	},
	TypeMapping {
		pattern: "Option<u32>",
		display_name: "u32",
		method_name: "property_number_option",
		is_required: false,
		generic_param: Some("u32"),
		generic_param2: None,
		is_enum: false,
	},
	TypeMapping {
		pattern: "Option<f64>",
		display_name: "f64",
		method_name: "property_number_option",
		is_required: false,
		generic_param: Some("f64"),
		generic_param2: None,
		is_enum: false,
	},
	TypeMapping {
		pattern: "Option<[f64;3]>",
		display_name: "[f64,f64,f64]",
		method_name: "property_number_array_option",
		is_required: false,
		generic_param: Some("f64"),
		generic_param2: Some("3"),
		is_enum: false,
	},
	TypeMapping {
		pattern: "Option<[f64;4]>",
		display_name: "[f64,f64,f64,f64]",
		method_name: "property_number_array_option",
		is_required: false,
		generic_param: Some("f64"),
		generic_param2: Some("4"),
		is_enum: false,
	},
	TypeMapping {
		pattern: "Option<[u8;3]>",
		display_name: "[u8,u8,u8]",
		method_name: "property_number_array_option",
		is_required: false,
		generic_param: Some("u8"),
		generic_param2: Some("3"),
		is_enum: false,
	},
	TypeMapping {
		pattern: "Option<Vec<String>>",
		display_name: "[String,...]",
		method_name: "property_string_list_option",
		is_required: false,
		generic_param: None,
		generic_param2: None,
		is_enum: false,
	},
	TypeMapping {
		pattern: "Option<TileCompression>",
		display_name: "TileCompression",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("TileCompression"),
		generic_param2: None,
		is_enum: true,
	},
	TypeMapping {
		pattern: "Option<TileSchema>",
		display_name: "TileSchema",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("TileSchema"),
		generic_param2: None,
		is_enum: true,
	},
	TypeMapping {
		pattern: "Option<TileFormat>",
		display_name: "TileFormat",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("TileFormat"),
		generic_param2: None,
		is_enum: true,
	},
	TypeMapping {
		pattern: "Option<PointReductionStrategy>",
		display_name: "PointReductionStrategy",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("PointReductionStrategy"),
		generic_param2: None,
		is_enum: true,
	},
	TypeMapping {
		pattern: "Option<RasterTileFormat>",
		display_name: "RasterTileFormat",
		method_name: "property_enum_option",
		is_required: false,
		generic_param: Some("RasterTileFormat"),
		generic_param2: None,
		is_enum: true,
	},
];

/// Find a type mapping by its pattern string.
fn find_type_mapping(type_str: &str) -> Option<&'static TypeMapping> {
	TYPE_MAPPINGS.iter().find(|m| m.pattern == type_str)
}

/// Generate the list of supported types for error messages.
fn supported_types_list() -> String {
	TYPE_MAPPINGS.iter().map(|m| m.pattern).collect::<Vec<_>>().join(", ")
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

/// Extract doc comments from struct attributes, preserving blank lines so the
/// rendered markdown keeps headings and code fences legible. `extract_comment`
/// collapses blank `///` lines to `None`; here we promote them to empty strings
/// so `join("\n")` yields real paragraph breaks.
fn extract_struct_docs(attrs: &[Attribute]) -> String {
	attrs
		.iter()
		.filter_map(|a| {
			if a.path().is_ident("doc") {
				Some(extract_comment(a).unwrap_or_default())
			} else {
				None
			}
		})
		.collect::<Vec<String>>()
		.join("\n")
		.trim()
		.to_string()
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
}

/// Processed field information returned by `process_field`.
enum ProcessedField {
	/// A regular property field with (doc_field, parser_field)
	Property { doc: String, parser: TokenStream },
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
		};
		return Ok((field_str, ProcessedField::Sources { doc, parser }, meta));
	}

	let mut comment = raw_comment.clone();
	if !comment.is_empty() {
		comment = format!(" - {comment}");
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

	let doc = if mapping.is_required {
		format!("- **`{field_str}`: {} (required)**{comment}", mapping.display_name)
	} else {
		format!("- *`{field_str}`: {} (optional)*{comment}", mapping.display_name)
	};

	let meta = FieldMeta {
		name: field_str.clone(),
		rust_type: field_type_str,
		is_required: mapping.is_required,
		is_sources: false,
		doc: raw_comment,
		enum_type: if mapping.is_enum { mapping.generic_param } else { None },
	};

	Ok((
		field_str,
		ProcessedField::Property {
			doc: doc.trim().to_string(),
			parser: quote! { #field_name: #call },
		},
		meta,
	))
}

/// Build the final impl TokenStream for the struct.
fn build_impl_tokens(
	name: &Ident,
	field_names: &[String],
	parser_fields: &[TokenStream],
	doc: &str,
	field_metas: &[FieldMeta],
) -> TokenStream {
	let meta_entries: Vec<TokenStream> = field_metas
		.iter()
		.map(|m| {
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
			quote! {
				crate::vpl::VPLFieldMeta {
					name: #fname.to_string(),
					rust_type: #rtype.to_string(),
					is_required: #required,
					is_sources: #is_sources,
					doc: #fdoc.to_string(),
					enum_variants: #variants_expr,
				}
			}
		})
		.collect();

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
				#doc.to_string()
			}

			#[cfg(feature = "codegen")]
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
	let mut doc_fields: Vec<String> = Vec::new();
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

	let doc_fields_str = if doc_fields.is_empty() {
		String::new()
	} else {
		format!("### Parameters\n\n{}", doc_fields.join("\n"))
	};

	let doc = vec![doc_struct, doc_sources.unwrap_or_default(), doc_fields_str]
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
		&doc,
		&field_metas,
	))
}

#[cfg(test)]
mod tests {
	use super::decode_struct;
	use syn::{DeriveInput, parse_quote};

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
			"property_number_array_required::<f64>",
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
}

use crate::decode_vpl::extract_comment;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{DataStruct, DeriveInput, Fields};

pub fn decode_struct(input: DeriveInput, data_struct: DataStruct) -> TokenStream {
	let name = input.ident;

	// Extract the doc comments from the struct attributes
	let doc_struct = input
		.attrs
		.iter()
		.filter_map(extract_comment)
		.collect::<Vec<String>>()
		.join("\n")
		.trim()
		.to_string();

	let fields = if let Fields::Named(fields_named) = data_struct.fields {
		fields_named.named
	} else {
		panic!("VPLDecode can only be derived for structs with named fields");
	};

	let mut parser_fields: Vec<TokenStream> = Vec::new();
	let mut doc_fields: Vec<String> = Vec::new();
	let mut doc_sources: Option<String> = None;
	let mut field_names: Vec<String> = Vec::new();

	for field in fields {
		let field_name = &field.ident;
		let field_type = &field.ty;
		let field_str = field_name.as_ref().expect("could not get field_name").to_string();
		let field_type_str = quote!(#field_type).to_string().replace(' ', "");

		field_names.push(field_str.clone());
		let mut comment = field
			.attrs
			.iter()
			.filter_map(extract_comment)
			.collect::<Vec<String>>()
			.join(" ")
			.trim()
			.to_string();

		if field_str == "sources" {
			if doc_sources.is_some() {
				panic!("'sources' are already defined: {doc_sources:?}")
			}
			if field_type_str != "Vec<VPLPipeline>" {
				panic!("type of 'sources' must be 'Vec<VPLPipeline>', but is '{field_type_str}'")
			}
			doc_sources = Some(format!("### Sources:\n{comment}"));
			parser_fields.push(quote! { sources: node.sources.clone() });
		} else {
			if !comment.is_empty() {
				comment = format!(" - {comment}");
			}
			let (doc_field, parser_field) = match field_type_str.as_str() {
				"String" => (
					format!("- **`{field_str}`: String (required)**{comment}"),
					quote! { #field_name: node.get_property_string_req(#field_str)? },
				),
				"bool" => (
					format!("- **`{field_str}`: Boolean (required)**{comment}"),
					quote! { #field_name: node.get_property_bool_req(#field_str)? },
				),
				"u8" => (
					format!("- **`{field_str}`: u8 (required)**{comment}"),
					quote! { #field_name: node.get_property_number_req::<u8>(#field_str)? },
				),
				"[f64;4]" => (
					format!("- **`{field_str}`: [f64,f64,f64,f64] (required)**{comment}"),
					quote! { #field_name: node.get_property_number_array4_req::<f64>(#field_str)? },
				),
				"Option<bool>" => (
					format!("- *`{field_str}`: bool (optional)*{comment}"),
					quote! { #field_name: node.get_property_bool(#field_str)? },
				),
				"Option<String>" => (
					format!("- *`{field_str}`: String (optional)*{comment}"),
					quote! { #field_name: node.get_property_string(#field_str)? },
				),
				"Option<f32>" => (
					format!("- *`{field_str}`: f32 (optional)*{comment}"),
					quote! { #field_name: node.get_property_number::<f32>(#field_str)? },
				),
				"Option<u8>" => (
					format!("- *`{field_str}`: u8 (optional)*{comment}"),
					quote! { #field_name: node.get_property_number::<u8>(#field_str)? },
				),
				"Option<u32>" => (
					format!("- *`{field_str}`: u32 (optional)*{comment}"),
					quote! { #field_name: node.get_property_number::<u32>(#field_str)? },
				),
				"Option<[f64;4]>" => (
					format!("- *`{field_str}`: [f64,f64,f64,f64] (optional)*{comment}"),
					quote! { #field_name: node.get_property_number_array4::<f64>(#field_str)? },
				),
				"Option<TileFormat>" => (
					format!("- *`{field_str}`: TileFormat (optional)*{comment}"),
					quote! { #field_name: node.get_property_enum::<TileFormat>(#field_str)? },
				),
				_ => panic!("unknown type field: {field_type_str}"),
			};
			doc_fields.push(doc_field.trim().to_string());
			parser_fields.push(parser_field);
		}
	}

	let doc_fields = if doc_fields.is_empty() {
		String::from("")
	} else {
		format!("### Parameters:\n{}", doc_fields.join("\n"))
	};

	let doc = vec![doc_struct, doc_sources.unwrap_or_default(), doc_fields]
		.into_iter()
		.filter(|s| !s.is_empty())
		.collect::<Vec<String>>()
		.join("\n")
		.trim()
		.to_string();

	quote! {
		impl #name {
			pub fn from_vpl_node(node: &VPLNode) -> Result<Self> {
				// scan node.get_property_names to ensure, that all properties are also defined in field_names
				let argument_names: Vec<String> = vec![#(#field_names.to_string()),*];
				let property_names = node.get_property_names();
				for property_name in property_names {
					if !argument_names.contains(&property_name) {
						anyhow::bail!("Unknown argument \"{}\" in \"{}\"", property_name, node.name);
					}
				}

				Ok(Self {
					#(#parser_fields),*
				})
			}

			pub fn get_docs() -> String {
				#doc.to_string()
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::decode_struct;
	use pretty_assertions::assert_eq;
	use syn::{DeriveInput, parse_quote};

	fn pretty_tokens(ts: proc_macro2::TokenStream) -> Vec<String> {
		prettyplease::unparse(&syn::parse_file(&ts.to_string()).unwrap())
			.split("\n")
			.map(|s| s.to_string())
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
		let ts = decode_struct(input.clone(), data_struct);
		assert_eq!(
			pretty_tokens(ts),
			[
				"impl Test {",
				"    pub fn from_vpl_node(node: &VPLNode) -> Result<Self> {",
				"        let argument_names: Vec<String> = vec![\"field1\".to_string()];",
				"        let property_names = node.get_property_names();",
				"        for property_name in property_names {",
				"            if !argument_names.contains(&property_name) {",
				"                anyhow::bail!(",
				"                    \"Unknown argument \\\"{}\\\" in \\\"{}\\\"\", property_name, node.name",
				"                );",
				"            }",
				"        }",
				"        Ok(Self {",
				"            field1: node.get_property_string_req(\"field1\")?,",
				"        })",
				"    }",
				"    pub fn get_docs() -> String {",
				"        \"Struct documentation\\n### Parameters:\\n- **`field1`: String (required)** - Field documentation\"",
				"            .to_string()",
				"    }",
				"}",
				""
			]
		);
	}

	#[test]
	fn test_decode_struct_all_field_types() {
		use syn::parse_quote;
		// Struct covering all supported field types
		let cases: Vec<(DeriveInput, &str, &str)> = vec![
			(
				parse_quote!(
					struct T {
						v: String,
					}
				),
				"get_property_string_req",
				"**`v`: String (required)**",
			),
			(
				parse_quote!(
					struct T {
						v: bool,
					}
				),
				"get_property_bool_req",
				"**`v`: Boolean (required)**",
			),
			(
				parse_quote!(
					struct T {
						v: u8,
					}
				),
				"get_property_number_req::<u8>",
				"**`v`: u8 (required)**",
			),
			(
				parse_quote!(
					struct T {
						v: [f64; 4],
					}
				),
				"get_property_number_array4_req::<f64>",
				"**`v`: [f64,f64,f64,f64] (required)**",
			),
			(
				parse_quote!(
					struct T {
						v: Option<bool>,
					}
				),
				"get_property_bool",
				"*`v`: bool (optional)*",
			),
			(
				parse_quote!(
					struct T {
						v: Option<String>,
					}
				),
				"get_property_string",
				"*`v`: String (optional)*",
			),
			(
				parse_quote!(
					struct T {
						v: Option<f32>,
					}
				),
				"get_property_number::<f32>",
				"*`v`: f32 (optional)*",
			),
			(
				parse_quote!(
					struct T {
						v: Option<u8>,
					}
				),
				"get_property_number::<u8>",
				"*`v`: u8 (optional)*",
			),
			(
				parse_quote!(
					struct T {
						v: Option<u32>,
					}
				),
				"get_property_number::<u32>",
				"*`v`: u32 (optional)*",
			),
			(
				parse_quote!(
					struct T {
						v: Option<[f64; 4]>,
					}
				),
				"get_property_number_array4::<f64>",
				"*`v`: [f64,f64,f64,f64] (optional)*",
			),
			(
				parse_quote!(
					struct T {
						v: Option<TileFormat>,
					}
				),
				"get_property_enum::<TileFormat>",
				"*`v`: TileFormat (optional)*",
			),
		];

		for (input, getter, comment) in cases {
			let data_struct = match &input.data {
				syn::Data::Struct(ds) => ds.clone(),
				_ => panic!("Expected struct data"),
			};
			let ts = decode_struct(input.clone(), data_struct);
			assert_eq!(
				pretty_tokens(ts),
				[
					"impl T {",
					"    pub fn from_vpl_node(node: &VPLNode) -> Result<Self> {",
					"        let argument_names: Vec<String> = vec![\"v\".to_string()];",
					"        let property_names = node.get_property_names();",
					"        for property_name in property_names {",
					"            if !argument_names.contains(&property_name) {",
					"                anyhow::bail!(",
					"                    \"Unknown argument \\\"{}\\\" in \\\"{}\\\"\", property_name, node.name",
					"                );",
					"            }",
					"        }",
					"        Ok(Self {",
					&format!("            v: node.{getter}(\"v\")?,"),
					"        })",
					"    }",
					"    pub fn get_docs() -> String {",
					&format!("        \"### Parameters:\\n- {comment}\".to_string()"),
					"    }",
					"}",
					""
				]
			)
		}
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
		let ts = decode_struct(input.clone(), data_struct);
		let code = ts.to_string();
		// Ensure get_docs includes Sources section
		assert!(code.contains("### Sources:"));
		assert!(code.contains("List of sources"));
	}
}

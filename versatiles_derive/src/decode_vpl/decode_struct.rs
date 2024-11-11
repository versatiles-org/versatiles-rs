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
		.join("\n");

	let fields = if let Fields::Named(fields_named) = data_struct.fields {
		fields_named.named
	} else {
		panic!("VPLDecode can only be derived for structs with named fields");
	};

	let mut parser_fields: Vec<TokenStream> = Vec::new();
	let mut doc_fields: Vec<String> = Vec::new();
	let mut doc_sources: Option<String> = None;

	for field in fields {
		let field_name = &field.ident;
		let field_type = &field.ty;
		let field_str = field_name
			.as_ref()
			.expect("could not get field_name")
			.to_string();
		let field_type_str = quote!(#field_type).to_string().replace(' ', "");

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
			doc_sources = Some(format!("### Sources:\n{comment}\n"));
			parser_fields.push(quote! { sources: node.sources.clone() });
		} else {
			if !comment.is_empty() {
				comment = format!(" - {comment}");
			}
			let (doc_field, parser_field) = match field_type_str.as_str() {
				"String" => (
					format!("* **`{field_str}`: String (required)**{comment}"),
					quote! { #field_name: node.get_property_string_req(#field_str)? },
				),
				"bool" => (
					format!("* *`{field_str}`: Boolean (optional, default: false)*{comment}"),
					quote! { #field_name: node.get_property_bool_req(#field_str)? },
				),
				"u8" => (
					format!("* *`{field_str}`: u8 *{comment}"),
					quote! { #field_name: node.get_property_number_req::<u8>(#field_str)? },
				),
				"[f64;4]" => (
					format!("* **`{field_str}`: [f64,f64,f64,f64] (required)**{comment}"),
					quote! { #field_name: node.get_property_number_array4_req::<f64>(#field_str)? },
				),
				"Option<String>" => (
					format!("* *`{field_str}`: String (optional)*{comment}"),
					quote! { #field_name: node.get_property_string(#field_str)? },
				),
				"Option<f32>" => (
					format!("* *`{field_str}`: f32 (optional)*{comment}"),
					quote! { #field_name: node.get_property_number::<f32>(#field_str)? },
				),
				"Option<u8>" => (
					format!("* *`{field_str}`: u8 (optional)*{comment}"),
					quote! { #field_name: node.get_property_number::<u8>(#field_str)? },
				),
				"Option<u32>" => (
					format!("* *`{field_str}`: u32 (optional)*{comment}"),
					quote! { #field_name: node.get_property_number::<u32>(#field_str)? },
				),
				"Option<[f64;4]>" => (
					format!("* *`{field_str}`: [f64,f64,f64,f64] (optional)*{comment}"),
					quote! { #field_name: node.get_property_number_array4::<f64>(#field_str)? },
				),
				_ => panic!("unknown type field: {field_type_str}"),
			};
			doc_fields.push(doc_field.trim().to_string());
			parser_fields.push(parser_field);
		}
	}

	let doc_children = doc_sources.unwrap_or_default();

	let doc_fields = if doc_fields.is_empty() {
		String::from("")
	} else {
		format!("### Parameters:\n{}", doc_fields.join("\n"))
	};

	quote! {
		impl #name {
			pub fn from_vpl_node(node: &VPLNode) -> Result<Self> {
				Ok(Self {
					#(#parser_fields),*
				})
			}

			pub fn get_docs() -> String {
				vec![
					&format!("{}\n", #doc_struct),
					#doc_fields,
					#doc_children,
				].join("").trim().to_string()
			}
		}
	}
}

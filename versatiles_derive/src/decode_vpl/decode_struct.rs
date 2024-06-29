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
	let mut doc_children: Option<String> = None;

	for field in fields {
		let field_name = &field.ident;
		let field_type = &field.ty;
		let field_str = field_name
			.as_ref()
			.expect("could not get field_name")
			.to_string();
		let field_type_str = quote!(#field_type).to_string();

		let mut comment = field
			.attrs
			.iter()
			.filter_map(extract_comment)
			.collect::<Vec<String>>()
			.join(" ");

		if !comment.is_empty() {
			comment = format!(" - {comment}");
		}

		if field_str == "children" {
			if doc_children.is_some() {
				panic!("'children' are already defined: {doc_children:?}")
			}
			if field_type_str != "Vec < VPLPipeline >" {
				panic!("type of 'children' must be 'Vec<VPLPipeline>', but is '{field_type_str}'")
			}
			doc_children = Some(format!("### Children:\n{comment}\n"));
			parser_fields.push(quote! { children: node.children.clone() });
		} else {
			let (doc_field, parser_field) = match field_type_str.as_str() {
				"String" => (
					format!("* **`{field_str}`: String (required)**{comment}"),
					quote! { #field_name: node.get_property_string1(#field_str)? },
				),
				"bool" => (
					format!("* *`{field_str}`: Boolean (optional, default: false)*{comment}"),
					quote! { #field_name: node.get_property_bool(#field_str)? },
				),
				"Option < String >" => (
					format!("* *`{field_str}`: String (optional)*{comment}"),
					quote! { #field_name: node.get_property_string0(#field_str)? },
				),
				"Option < f32 >" => (
					format!("* *`{field_str}`: f32 (optional)*{comment}"),
					quote! { #field_name: node.get_property_number0::<f32>(#field_str)? },
				),
				"Option < u8 >" => (
					format!("* *`{field_str}`: u8 (optional)*{comment}"),
					quote! { #field_name: node.get_property_number0::<u8>(#field_str)? },
				),
				"Option < u32 >" => (
					format!("* *`{field_str}`: u32 (optional)*{comment}"),
					quote! { #field_name: node.get_property_number0::<u32>(#field_str)? },
				),
				_ => panic!("unknown type field: {field_type_str}"),
			};
			doc_fields.push(doc_field);
			parser_fields.push(parser_field);
		}
	}

	let doc_children = doc_children.unwrap_or_default();

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
				].join("")
			}
		}
	}
}
use crate::kdl::Definition;
use proc_macro::TokenStream;
use quote::quote;
use syn::{Attribute, Data, Fields, Meta};

pub fn get_doc(input: &Definition) -> TokenStream {
	let name = input.ident;

	fn extract_comment(attr: &Attribute) -> Option<String> {
		if attr.path().is_ident("doc") {
			if let Meta::NameValue(meta) = &attr.meta {
				if let syn::Expr::Lit(lit) = &meta.value {
					if let syn::Lit::Str(lit_str) = &lit.lit {
						let text = lit_str.value().trim().to_string();
						return if text.is_empty() { None } else { Some(text) };
					}
				}
			}
		}
		None
	}

	// Extract the doc comments from the struct attributes
	let struct_docs = input
		.attrs
		.iter()
		.filter_map(extract_comment)
		.collect::<Vec<String>>()
		.join("\n");

	let fields = if let Data::Struct(data_struct) = input.data {
		if let Fields::Named(fields_named) = data_struct.fields {
			fields_named.named
		} else {
			panic!("YamlParser can only be derived for structs with named fields");
		}
	} else {
		panic!("YamlParser can only be derived for structs");
	};

	let field_docs: Vec<_> = fields
		.iter()
		.map(|field| {
			let field_name = &field.ident;
			let field_type = &field.ty;
			let field_str = field_name.as_ref().unwrap().to_string();

			let field_doc = match quote!(#field_type).to_string().as_str() {
				"String" => format!("**`{field_str}`: String (required)**"),
				"bool" => format!("*`{field_str}`: Boolean (optional, default: false)*"),
				"Option < String >" => format!("*`{field_str}`: String (optional)*"),
				_ => format!("unknown format `{field_type:?}`"),
			};

			let comment = field
				.attrs
				.iter()
				.filter_map(extract_comment)
				.collect::<Vec<String>>()
				.join(" ");

			let field_doc = if comment.is_empty() {
				quote! { docs.push_str(&format!("* {}\n", #field_doc)); }
			} else {
				quote! { docs.push_str(&format!("* {} - {}\n", #field_doc, #comment)); }
			};

			field_doc
		})
		.collect();

	quote! {
		impl OperationDocsTrait for #name {
			pub fn generate_docs() -> String {
				let mut docs = String::new();
				docs.push_str(&format!("{}\n", #struct_docs));
				docs.push_str("### Arguments:\n");
				#(#field_docs)*
				docs
			}
		}
	}
}

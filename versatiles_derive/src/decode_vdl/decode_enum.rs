use crate::decode_vdl::camel_to_snake;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{DataEnum, DeriveInput, Fields};

pub fn decode_enum(input: DeriveInput, data_enum: DataEnum) -> TokenStream {
	let name = input.ident;

	let mut get_docs = Vec::new();
	let mut variants = Vec::new();

	for variant in data_enum.variants {
		let variant_name = &variant.ident;
		let variant_type = if let Fields::Unnamed(fields) = &variant.fields {
			if fields.unnamed.len() == 1 {
				&fields
					.unnamed
					.first()
					.expect("could not get first unnamed field")
					.ty
			} else {
				panic!("VDLDecode can only be derived for enums with single unnamed field variants");
			}
		} else {
			panic!("VDLDecode can only be derived for enums with unnamed fields");
		};
		let node_name = camel_to_snake(&variant_name.to_string());

		variants.push(quote! {
			if node.name == #node_name {
				return Ok(Self::#variant_name(#variant_type ::from_vdl_node(node)?));
			}
		});

		let headline = format!("\n## {node_name}");

		get_docs.push(quote! {
			String::from(#headline),
			#variant_type ::get_docs(),
		});
	}

	quote! {
		impl #name {
			pub fn from_vdl_node(node: &VDLNode) -> Result<#name> {
				#(#variants)*
				Err(anyhow::anyhow!("Unknown variant: {}", node.name))
			}

			pub fn get_docs() -> String {
				vec![
					#(#get_docs)*
				].join("\n")
			}
		}
	}
}

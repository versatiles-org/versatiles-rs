extern crate proc_macro;
extern crate quote;
extern crate syn;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

#[proc_macro_derive(YamlParser)]
pub fn yaml_parser_derive(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as DeriveInput);
	let name = input.ident;

	let fields = if let Data::Struct(data_struct) = input.data {
		if let Fields::Named(fields_named) = data_struct.fields {
			fields_named.named
		} else {
			panic!("YamlParser can only be derived for structs with named fields");
		}
	} else {
		panic!("YamlParser can only be derived for structs");
	};

	let field_parsing = fields.iter().map(|field| {
        let field_name = &field.ident;
        let field_type = &field.ty;
           let field_str = field_name.as_ref().unwrap().to_string();

        match quote!(#field_type).to_string().as_str() {
            "String" => quote! {
                #field_name: yaml.hash_get_string(#field_str).context(format!("Failed to get '{}'", #field_str))?
            },
            "bool" => quote! {
                #field_name: yaml.hash_get_bool(#field_str).unwrap_or(false)
            },
            "Option < String >" => quote! {
                #field_name: yaml.hash_get_string(#field_str).ok()
            },
            _ => quote! {
                #field_name: yaml.hash_get_str(#field_str).context(format!("Failed to get '{}'", #field_str))?.parse::<#field_type>().context(format!("Failed to parse '{}'", #field_str))?
            },
        }
    });

	let expanded = quote! {
		 impl #name {
			  pub fn from_yaml(yaml: &YamlWrapper) -> anyhow::Result<Self> {
					Ok(Self {
						 #(#field_parsing),*
					})
			  }
		 }
	};

	TokenStream::from(expanded)
}

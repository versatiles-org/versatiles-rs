mod args;
mod config_doc;
mod decode_vpl;

use crate::{args::*, config_doc::*, decode_vpl::*};
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::{ToTokens, quote};
use syn::{Fields, parse_macro_input, spanned::Spanned};

#[proc_macro_derive(VPLDecode)]
pub fn decode_vpl(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as syn::DeriveInput);

	let expanded = match input.data.clone() {
		syn::Data::Struct(data_struct) => decode_struct(input, data_struct),
		_ => panic!("VPLDecode can only be derived for structs, but: {:?}", input.data),
	};

	TokenStream::from(expanded)
}

#[proc_macro_derive(ConfigDoc, attributes(config))]
pub fn derive_config_doc(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as syn::DeriveInput);
	let name = &input.ident;

	// Collect struct-level docs
	let struct_doc = collect_doc(&input.attrs);

	// Ensure it's a named-field struct and gather fields
	let data = match &input.data {
		syn::Data::Struct(ds) => ds,
		_ => {
			return syn::Error::new(
				input.span(),
				"ConfigDoc can only be derived for structs with named fields",
			)
			.to_compile_error()
			.into();
		}
	};

	let fields = match &data.fields {
		Fields::Named(named) => &named.named,
		_ => {
			return syn::Error::new(
				data.struct_token.span(),
				"ConfigDoc requires a struct with named fields",
			)
			.to_compile_error()
			.into();
		}
	};

	// Precompute per-field metadata for codegen
	let mut rows = Vec::new();
	for f in fields {
		let ident = f.ident.as_ref().expect("named field");
		let key = serde_rename(&f.attrs).unwrap_or_else(|| ident.to_string());
		let ty_tokens = f.ty.to_token_stream().to_string();
		let optional = is_option(&f.ty);
		let doc = collect_doc(&f.attrs);

		rows.push((key, ty_tokens, optional, doc));
	}

	// Build code that writes the Markdown at runtime
	let keys = rows.iter().map(|r| &r.0);
	let tys = rows.iter().map(|r| &r.1);
	let optionals = rows.iter().map(|r| if r.2 { "yes" } else { "no" });
	let docs = rows.iter().map(|r| &r.3);

	let expanded = quote! {
		 impl #name {
			  pub fn md() -> String {
					let mut __s = String::new();
					__s.push_str(&format!("# {}\n\n", stringify!(#name)));
					if !#struct_doc.is_empty() {
						 __s.push_str(#struct_doc);
						 __s.push('\n');
						 __s.push('\n');
					}
					__s.push_str("| Key | Type | Optional | Description |\n");
					__s.push_str("| --- | ---- | -------- | ----------- |\n");
					#( {
						 __s.push_str("| `");
						 __s.push_str(#keys);
						 __s.push_str("` | `");
						 __s.push_str(#tys);
						 __s.push_str("` | ");
						 __s.push_str(#optionals);
						 __s.push_str(" | ");
						 if !#docs.is_empty() {
							  __s.push_str(#docs);
						 } else {
							  __s.push_str("—");
						 }
						 __s.push_str(" |\n");
					} )*
					__s
			  }
		 }
	};

	TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn context(args: TokenStream, input: TokenStream) -> TokenStream {
	let Args(move_token, format_args) = parse_macro_input!(args);
	let mut input = parse_macro_input!(input as syn::ItemFn);

	let body = &input.block;
	let return_type = &input.sig.output;
	let err = Ident::new("err", Span::mixed_site());

	let new_body = if input.sig.asyncness.is_some() {
		let return_type = match return_type {
			syn::ReturnType::Default => {
				return syn::Error::new_spanned(input, "function should return Result")
					.to_compile_error()
					.into();
			}
			syn::ReturnType::Type(_, return_type) => return_type,
		};
		let result = Ident::new("result", Span::mixed_site());
		quote! {
			let #result: #return_type = async #move_token { #body }.await;
			#result.map_err(|#err| #err.context(format!(#format_args)).into())
		}
	} else {
		let force_fn_once = Ident::new("force_fn_once", Span::mixed_site());
		quote! {
			// Moving a non-`Copy` value into the closure tells borrowck to always treat the closure
			// as a `FnOnce`, preventing some borrowing errors.
			let #force_fn_once = ::core::iter::empty::<()>();
			(#move_token || #return_type {
				::core::mem::drop(#force_fn_once);
				#body
			})().map_err(|#err| #err.context(format!(#format_args)).into())
		}
	};
	input.block.stmts = vec![syn::Stmt::Expr(syn::Expr::Verbatim(new_body), None)];

	input.into_token_stream().into()
}

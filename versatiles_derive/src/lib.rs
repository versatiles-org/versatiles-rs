mod args;
mod decode_vpl;

use crate::args::Args;
use decode_vpl::decode_struct;
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::{ToTokens, quote};
use syn::parse_macro_input;

#[proc_macro_derive(VPLDecode)]
pub fn decode_vpl(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as syn::DeriveInput);

	let expanded = match input.data.clone() {
		syn::Data::Struct(data_struct) => decode_struct(input, data_struct),
		_ => panic!("VPLDecode can only be derived for structs, but: {:?}", input.data),
	};

	TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn context(args: TokenStream, input: TokenStream) -> TokenStream {
	let Args(move_token, format_args) = parse_macro_input!(args);
	let mut input = parse_macro_input!(input as syn::ItemFn);

	let body = &input.block;
	let return_ty = &input.sig.output;
	let err = Ident::new("err", Span::mixed_site());

	let new_body = if input.sig.asyncness.is_some() {
		let return_ty = match return_ty {
			syn::ReturnType::Default => {
				return syn::Error::new_spanned(input, "function should return Result")
					.to_compile_error()
					.into();
			}
			syn::ReturnType::Type(_, return_ty) => return_ty,
		};
		let result = Ident::new("result", Span::mixed_site());
		quote! {
			let #result: #return_ty = async #move_token { #body }.await;
			#result.map_err(|#err| #err.context(format!(#format_args)).into())
		}
	} else {
		let force_fn_once = Ident::new("force_fn_once", Span::mixed_site());
		quote! {
			// Moving a non-`Copy` value into the closure tells borrowck to always treat the closure
			// as a `FnOnce`, preventing some borrowing errors.
			let #force_fn_once = ::core::iter::empty::<()>();
			(#move_token || #return_ty {
				::core::mem::drop(#force_fn_once);
				#body
			})().map_err(|#err| #err.context(format!(#format_args)).into())
		}
	};
	input.block.stmts = vec![syn::Stmt::Expr(syn::Expr::Verbatim(new_body), None)];

	input.into_token_stream().into()
}

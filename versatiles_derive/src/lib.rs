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

#[proc_macro_derive(ConfigDoc, attributes(config, config_demo))]
pub fn derive_config_doc(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as syn::DeriveInput);
	let name = &input.ident;

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

	// Per-field computed metadata for codegen
	struct Row {
		ident: syn::Ident,
		key: String,
		ty: syn::Type,
		doc: String,
		is_vec: bool,
		is_map: bool,
		is_flatten: bool,
		inner_ty_vec: Option<syn::Type>, // Vec<T> -> T
		// Heuristic: treat non-Option/Vec/Map path types as nested
		is_nested_struct: bool,
		is_url_path: bool,
		demo_value: Option<String>,
	}

	let mut rows = Vec::<Row>::new();
	for f in fields {
		let ident = f.ident.clone().expect("named field");
		let key = serde_rename(&f.attrs).unwrap_or_else(|| ident.to_string());
		let ty = f.ty.clone();
		let doc = collect_doc(&f.attrs);
		let is_option = is_option(&f.ty);

		let mut is_vec = false;
		let mut is_map = false;
		let mut inner_ty_vec = None;

		// classify Option / Vec / HashMap
		if let Some(id) = path_ident(&f.ty) {
			let id_s = id.to_string();
			if id_s == "Vec" {
				is_vec = true;
				if let Some(mut inners) = angle_inner(&f.ty) {
					if let Some(inner) = inners.pop() {
						inner_ty_vec = Some(inner.clone());
					}
				}
			} else if id_s == "HashMap" {
				is_map = true;
			}
		}

		let is_flatten = has_serde_flatten(&f.attrs);

		let is_url_path = is_url_path(&f.ty);

		// parse demo from #[config_demo("...")] or #[config_demo(value = "...")]
		let mut demo_value = None;
		for attr in &f.attrs {
			if attr.path().is_ident("config_demo") {
				// Prefer positional literal: #[config_demo("...")]
				if demo_value.is_none() {
					if let Ok(lit) = attr.parse_args::<syn::LitStr>() {
						demo_value = Some(lit.value());
						continue;
					}
				}
				// Fallback to name-value: #[config_demo(value = "...")]
				let _ = attr.parse_nested_meta(|meta| {
					if meta.path.is_ident("value") {
						if let Ok(v) = meta.value() {
							if let Ok(lit) = v.parse::<syn::LitStr>() {
								demo_value = Some(lit.value());
							}
						}
					}
					Ok(())
				});
			}
		}

		// decide if nested struct
		let is_nested_struct = !is_option
			&& !is_vec
			&& !is_map
			&& matches!(path_ident(&f.ty), Some(_))
			&& !is_primitive_like(&f.ty)
			&& !is_url_path;

		rows.push(Row {
			ident,
			key,
			ty,
			doc,
			is_vec,
			is_map,
			is_flatten,
			inner_ty_vec,
			is_nested_struct,
			is_url_path,
			demo_value,
		});
	}

	// Generate per-field YAML code blocks
	let field_yaml_blocks: Vec<_> = rows
		.iter()
		.map(|r| {
			use proc_macro2::TokenStream as TokenStream2;

			let _ident = &r.ident;
			let key = &r.key;
			let ty = &r.ty;
			let doc = &r.doc;
			let doc_lit = syn::LitStr::new(doc, Span::call_site());
			let demo_value = r.demo_value.as_ref();
			let demo_lit = demo_value.map(|d| syn::LitStr::new(d, Span::call_site()));

			let key_lit = syn::LitStr::new(key, Span::call_site());
			let emit_doc_block: TokenStream2 = if doc.is_empty() {
				quote! {}
			} else {
				quote! {
					for line in #doc_lit.lines() {
						__s.push_str(&__sp(__indent));
						__s.push_str("# ");
						__s.push_str(line);
						__s.push('\n');
					}
				}
			};

			let is_primitive = is_primitive_like(ty);

			let mut output: TokenStream2 = quote! {};

			output = quote! {
				#output
				#emit_doc_block
			};

			let indent_key = quote! {
				__s.push_str(&__sp(__indent));
				__s.push_str(#key_lit);
				__s.push_str(": ");
			};

			if r.is_flatten {
				output = quote! {
					#output
					__s.push_str(&<#ty>::demo_yaml_with_indent(__indent));
				};
			} else if r.is_nested_struct {
				output = quote! {
					#output
					#indent_key
					__s.push_str("\n");
					__s.push_str(&<#ty>::demo_yaml_with_indent(__indent + 2));
				};
			} else if r.is_vec {
				output = quote! {
					#output
					#indent_key
				};
				if let Some(demo_lit) = &demo_lit {
					let demo_trim = demo_lit.value().trim().to_string();
					if !demo_trim.starts_with('[') {
						output = quote! {
							#output
							__s.push_str("\n");
							__s.push_str(&__sp(__indent + 2));
							__s.push_str("-");
						};
					}
					output = quote! {
						#output
						__s.push_str(" ");
						__s.push_str(#demo_lit);
					};
				} else if let Some(inner) = &r.inner_ty_vec {
					let is_primitive_inner = is_primitive_like(inner);
					let is_url_path_inner = is_url_path(inner);
					output = quote! {
						#output
						__s.push_str("\n");
					};
					if is_primitive_inner {
						output = quote! {
							#output
							__s.push_str(&__sp(__indent + 2));
							__s.push_str("- ");
							let __v: #inner = ::core::default::Default::default();
							let __y = ::serde_yaml_ng::to_string(&__v).unwrap();
							__s.push_str(__y.trim());
						};
					} else if is_url_path_inner {
						output = quote! {
							#output
							__s.push_str(&__sp(__indent + 2));
							__s.push_str("- ");
							__s.push_str("\"\"");
						};
					} else {
						output = quote! {
							#output
							let __inner = <#inner>::demo_yaml_with_indent(0);
							let mut __first_line_printed = false;
							for __line in __inner.lines() {
								if !__first_line_printed {
									if __line.trim().is_empty() { continue; }
									__s.push_str(&__sp(__indent + 2));
									__s.push_str("- ");
									__first_line_printed = true;
								} else {
									__s.push_str(&__sp(__indent + 4));
								}
								__s.push_str(__line);
								__s.push('\n');
							}
						};
					}
				} else {
					output = quote! {
						#output
						__s.push_str("[]");
					};
				}
				output = quote! {
					#output
					__s.push_str("\n");
				};
			} else {
				// Unified leaf emission (Option, Map, UrlPath, scalar/non-primitive)
				output = quote! {
					#output
					#indent_key
				};
				if let Some(demo_lit) = &demo_lit {
					output = quote! { #output __s.push_str(#demo_lit); };
				} else {
					// Minimal sensible defaults when no demo is provided
					if r.is_map {
						output = quote! { #output __s.push_str("{}"); };
					} else if r.is_url_path {
						output = quote! { #output __s.push_str("\"\""); };
					} else if is_primitive {
						output = quote! {
							#output
							let __v: #ty = ::core::default::Default::default();
							let __y = ::serde_yaml_ng::to_string(&__v).unwrap();
							__s.push_str(__y.trim());
						};
					} else {
						// Generic non-primitive leaf: show an empty object to hint structure
						output = quote! { #output __s.push_str("{}"); };
					}
				}
				output = quote! { #output __s.push_str("\n"); };
			}

			output
		})
		.collect();

	let expanded = quote! {
		impl #name {
			pub fn demo_yaml() -> String {
				Self::demo_yaml_with_indent(0)
			}

			pub(crate) fn demo_yaml_with_indent(__indent: usize) -> String {
				let mut __s = String::new();
				let __sp = |n: usize| -> String { " ".repeat(n) };

				#( {
					#field_yaml_blocks
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

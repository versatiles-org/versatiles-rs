#![allow(dead_code, unused_variables)]

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

	// crude primitive-ish detection for deciding nested vs scalar
	fn is_primitive_like(ty: &syn::Type) -> bool {
		let s = ty.to_token_stream().to_string();
		matches!(
			s.as_str(),
			"bool"
				| "u8" | "u16"
				| "u32" | "u64"
				| "u128" | "i8"
				| "i16" | "i32"
				| "i64" | "i128"
				| "f32" | "f64"
				| "String"
				| "& str"
		)
	}

	fn path_ident(ty: &syn::Type) -> Option<&syn::Ident> {
		if let syn::Type::Path(tp) = ty {
			tp.path.segments.last().map(|s| &s.ident)
		} else {
			None
		}
	}

	fn angle_inner(ty: &syn::Type) -> Option<Vec<syn::Type>> {
		if let syn::Type::Path(tp) = ty {
			if let Some(seg) = tp.path.segments.last() {
				if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
					let mut v = Vec::new();
					for a in args.args.iter() {
						if let syn::GenericArgument::Type(t) = a {
							v.push(t.clone());
						}
					}
					return Some(v);
				}
			}
		}
		None
	}

	fn has_serde_flatten(attrs: &[syn::Attribute]) -> bool {
		for attr in attrs {
			if attr.path().is_ident("serde") {
				let mut found = false;
				let _ = attr.parse_nested_meta(|meta| {
					if meta.path.is_ident("flatten") {
						found = true;
					}
					Ok(())
				});
				if found {
					return true;
				}
			}
		}
		false
	}

	fn is_url_path(ty: &syn::Type) -> bool {
		if let syn::Type::Path(tp) = ty {
			if let Some(seg) = tp.path.segments.last() {
				return seg.ident == "UrlPath";
			}
		}
		false
	}

	// Per-field computed metadata for codegen
	struct Row {
		ident: syn::Ident,
		key: String,
		ty_tokens: String,
		ty: syn::Type,
		doc: String,
		is_option: bool,
		is_vec: bool,
		is_map: bool,
		is_flatten: bool,
		inner_ty_opt: Option<syn::Type>,                // Option<T> -> T
		inner_ty_vec: Option<syn::Type>,                // Vec<T> -> T
		_inner_tys_map: Option<(syn::Type, syn::Type)>, // HashMap<K,V> -> (K,V)
		// Heuristic: treat non-Option/Vec/Map path types as nested
		is_nested_struct: bool,
		is_url_path: bool,
		example_yaml: Option<String>,
		demo_value: Option<String>,
	}

	let mut rows = Vec::<Row>::new();
	for f in fields {
		let ident = f.ident.clone().expect("named field");
		let key = serde_rename(&f.attrs).unwrap_or_else(|| ident.to_string());
		let ty_tokens = f.ty.to_token_stream().to_string();
		let ty = f.ty.clone();
		let doc = collect_doc(&f.attrs);
		let is_option = is_option(&f.ty);

		let mut is_vec = false;
		let mut is_map = false;
		let mut inner_ty_opt = None;
		let mut inner_ty_vec = None;
		let mut _inner_tys_map = None;

		// classify Option / Vec / HashMap
		if let Some(id) = path_ident(&f.ty) {
			let id_s = id.to_string();
			if id_s == "Option" {
				if let Some(mut inners) = angle_inner(&f.ty) {
					if let Some(inner) = inners.pop() {
						inner_ty_opt = Some(inner.clone());
					}
				}
			} else if id_s == "Vec" {
				is_vec = true;
				if let Some(mut inners) = angle_inner(&f.ty) {
					if let Some(inner) = inners.pop() {
						inner_ty_vec = Some(inner.clone());
					}
				}
			} else if id_s == "HashMap" {
				is_map = true;
				if let Some(inners) = angle_inner(&f.ty) {
					if inners.len() == 2 {
						_inner_tys_map = Some((inners[0].clone(), inners[1].clone()));
					}
				}
			}
		}

		let is_flatten = has_serde_flatten(&f.attrs);

		let is_url_path = is_url_path(&f.ty);

		// parse example_yaml from #[config(example_yaml = r#"..."#)]
		let mut example_yaml = None;
		for attr in &f.attrs {
			if attr.path().is_ident("config") {
				let _ = attr.parse_nested_meta(|meta| {
					if meta.path.is_ident("example_yaml") {
						if let Ok(val) = meta.value() {
							if let Ok(lit) = val.parse::<syn::LitStr>() {
								example_yaml = Some(lit.value());
							}
						}
					}
					Ok(())
				});
			}
		}

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
			ty_tokens,
			ty,
			doc,
			is_option,
			is_vec,
			is_map,
			is_flatten,
			inner_ty_opt,
			inner_ty_vec,
			_inner_tys_map,
			is_nested_struct,
			is_url_path,
			example_yaml,
			demo_value,
		});
	}

	// Split rows into token streams
	let _idents: Vec<_> = rows.iter().map(|r| &r.ident).collect();
	let keys: Vec<_> = rows.iter().map(|r| r.key.as_str()).collect();
	let tys: Vec<_> = rows.iter().map(|r| r.ty_tokens.as_str()).collect();
	let _tys_types: Vec<_> = rows.iter().map(|r| &r.ty).collect();
	let docs: Vec<_> = rows.iter().map(|r| r.doc.as_str()).collect();
	let optionals = rows.iter().map(|r| if r.is_option { "yes" } else { "no" });
	let _example_yamls: Vec<_> = rows.iter().map(|r| r.example_yaml.as_ref()).collect();

	// Generate per-field YAML code blocks
	let field_yaml_blocks: Vec<_> = rows
		.iter()
		.map(|r| {
			let _ident = &r.ident;
			let key = &r.key;
			let ty = &r.ty;
			let doc = &r.doc;
			let doc_lit = syn::LitStr::new(doc, Span::call_site());
			let doc_trim_owned = doc.trim().to_string();
			let doc_trim_lit = syn::LitStr::new(&doc_trim_owned, Span::call_site());
			let example_yaml = r.example_yaml.as_ref();
			let example_yaml_lit = example_yaml.map(|ex| syn::LitStr::new(ex, Span::call_site()));
			let demo_value = r.demo_value.as_ref();
			let demo_lit = demo_value.map(|d| syn::LitStr::new(d, Span::call_site()));
			let inner_opt = r.inner_ty_opt.as_ref();
			let is_primitive_inner_opt = inner_opt.map_or(false, |ty| is_primitive_like(ty));
			let is_primitive = is_primitive_like(ty);

			let doc_lines = if doc.is_empty() {
				quote! {}
			} else {
				quote! {
					__emit_above_comment(&mut __s, __indent, #doc_lit);
				}
			};

			let example_emit = if let Some(ex_lit) = &example_yaml_lit {
				quote! {
					__emit_above_comment(&mut __s, __indent, #ex_lit);
				}
			} else {
				quote! {}
			};

			if r.is_flatten {
				quote! {
					#doc_lines
					__s.push_str(&<#ty>::demo_yaml_with_indent(__indent));
				}
			} else if r.is_nested_struct {
				quote! {
					#doc_lines
					__s.push_str(&__sp(__indent));
					__s.push_str(#key);
					__s.push_str(":\n");
					__s.push_str(&<#ty>::demo_yaml_with_indent(__indent + 2));
				}
			} else if r.is_option {
				if is_primitive_inner_opt {
					let demo_emit = if let Some(demo_lit) = &demo_lit {
						quote! {
							if #doc_trim_lit.is_empty() {
								__s.push_str(&__sp(__indent));
								__s.push_str(#key);
								__s.push_str(": ");
								__s.push_str(#demo_lit);
								__s.push('\n');
							} else if __should_inline_comment(#doc_lit) {
								__s.push_str(&__sp(__indent));
								__s.push_str(#key);
								__s.push_str(": ");
								__s.push_str(#demo_lit);
								__s.push_str("  # ");
								__s.push_str(#doc_trim_lit);
								__s.push('\n');
							} else {
								__emit_above_comment(&mut __s, __indent, #doc_lit);
								__s.push_str(&__sp(__indent));
								__s.push_str(#key);
								__s.push_str(": ");
								__s.push_str(#demo_lit);
								__s.push('\n');
							}
						}
					} else {
						let doc_emit = if doc.trim().is_empty() {
							quote! {
								__s.push_str(&__sp(__indent));
								__s.push_str(#key);
								__s.push_str(": <optional>\n");
							}
						} else {
							quote! {
								if __should_inline_comment(#doc_lit) {
									__s.push_str(&__sp(__indent));
									__s.push_str(#key);
									__s.push_str(": <optional>  # ");
									__s.push_str(#doc_trim_lit);
									__s.push('\n');
								} else {
									__emit_above_comment(&mut __s, __indent, #doc_lit);
									__s.push_str(&__sp(__indent));
									__s.push_str(#key);
									__s.push_str(": <optional>\n");
								}
							}
						};
						doc_emit
					};
					quote! {
						#example_emit
						#demo_emit
					}
				} else {
					let demo_emit = if let Some(demo_lit) = &demo_lit {
						quote! {
							#doc_lines
							__s.push_str(&__sp(__indent));
							__s.push_str(#key);
							__s.push_str(": ");
							__s.push_str(#demo_lit);
							__s.push('\n');
						}
					} else {
						quote! {
							#doc_lines
							__s.push_str(&__sp(__indent));
							__s.push_str(#key);
							__s.push_str(": <optional>\n");
						}
					};
					demo_emit
				}
			} else if r.is_vec {
				if let Some(demo_lit) = &demo_lit {
					let demo_trim = demo_lit.value().trim().to_string();
					if demo_trim.starts_with('[') {
						quote! {
							#example_emit
							#doc_lines
							__s.push_str(&__sp(__indent));
							__s.push_str(#key);
							__s.push_str(": ");
							__s.push_str(#demo_lit);
							__s.push('\n');
						}
					} else {
						quote! {
							#example_emit
							#doc_lines
							__s.push_str(&__sp(__indent));
							__s.push_str(#key);
							__s.push_str(":\n");
							__s.push_str(&__sp(__indent + 2));
							__s.push_str("- ");
							__s.push_str(#demo_lit);
							__s.push('\n');
						}
					}
				} else if let Some(inner) = &r.inner_ty_vec {
					let is_primitive_inner = is_primitive_like(inner);
					let is_url_path_inner = is_url_path(inner);
					if is_primitive_inner {
						quote! {
							#example_emit
							#doc_lines
							__s.push_str(&__sp(__indent));
							__s.push_str(#key);
							__s.push_str(":\n");
							__s.push_str(&__sp(__indent + 2));
							__s.push_str("- ");
							let __v: #inner = ::core::default::Default::default();
							let __y = ::serde_yaml_ng::to_string(&__v).unwrap();
							__s.push_str(__y.trim());
							__s.push('\n');
						}
					} else if is_url_path_inner {
						quote! {
							#example_emit
							#doc_lines
							__s.push_str(&__sp(__indent));
							__s.push_str(#key);
							__s.push_str(":\n");
							__s.push_str(&__sp(__indent + 2));
							__s.push_str("- ");
							__s.push_str("\"\"\n");
						}
					} else {
						quote! {
							#example_emit
							#doc_lines
							__s.push_str(&__sp(__indent));
							__s.push_str(#key);
							__s.push_str(":\n");
							let __inner = <#inner>::demo_yaml_with_indent(0);
							let mut __first_line_printed = false;
							for __line in __inner.lines() {
								if !__first_line_printed {
									if __line.trim().is_empty() {
										continue;
									}
									__s.push_str(&__sp(__indent + 2));
									__s.push_str("- ");
									__s.push_str(__line);
									__s.push('\n');
									__first_line_printed = true;
								} else {
									__s.push_str(&__sp(__indent + 4));
									__s.push_str(__line);
									__s.push('\n');
								}
							}
							if !__first_line_printed {
								// fallback if inner produced only empty lines
								__s.push_str(&__sp(__indent + 2));
								__s.push_str("- {}\n");
							}
						}
					}
				} else {
					quote! {
						#example_emit
						#doc_lines
						__s.push_str(&__sp(__indent));
						__s.push_str(#key);
						__s.push_str(": []\n");
					}
				}
			} else if r.is_map {
				if let Some(demo_lit) = &demo_lit {
					quote! {
						#doc_lines
						__s.push_str(&__sp(__indent));
						__s.push_str(#key);
						__s.push_str(": ");
						__s.push_str(#demo_lit);
						__s.push('\n');
					}
				} else {
					quote! {
						#doc_lines
						__s.push_str(&__sp(__indent));
						__s.push_str(#key);
						__s.push_str(": {}\n");
					}
				}
			} else if r.is_url_path {
				if let Some(demo_lit) = &demo_lit {
					quote! {
						#doc_lines
						__s.push_str(&__sp(__indent));
						__s.push_str(#key);
						__s.push_str(": ");
						__s.push_str(#demo_lit);
						__s.push('\n');
					}
				} else {
					quote! {
						#doc_lines
						__s.push_str(&__sp(__indent));
						__s.push_str(#key);
						__s.push_str(": \"\"\n");
					}
				}
			} else {
				// scalar/string primitive-like
				if is_primitive {
					if let Some(demo_lit) = &demo_lit {
						quote! {
							if #doc_trim_lit.is_empty() {
								__s.push_str(&__sp(__indent));
								__s.push_str(#key);
								__s.push_str(": ");
								__s.push_str(#demo_lit);
								__s.push('\n');
							} else if __should_inline_comment(#doc_lit) {
								__s.push_str(&__sp(__indent));
								__s.push_str(#key);
								__s.push_str(": ");
								__s.push_str(#demo_lit);
								__s.push_str("  # ");
								__s.push_str(#doc_trim_lit);
								__s.push('\n');
							} else {
								__emit_above_comment(&mut __s, __indent, #doc_lit);
								__s.push_str(&__sp(__indent));
								__s.push_str(#key);
								__s.push_str(": ");
								__s.push_str(#demo_lit);
								__s.push('\n');
							}
						}
					} else {
						quote! {
							if #doc_trim_lit.is_empty() {
								__s.push_str(&__sp(__indent));
								__s.push_str(#key);
								__s.push_str(": ");
								let __v: #ty = ::core::default::Default::default();
								let __y = ::serde_yaml_ng::to_string(&__v).unwrap();
								__s.push_str(__y.trim());
								__s.push('\n');
							} else if __should_inline_comment(#doc_lit) {
								__s.push_str(&__sp(__indent));
								__s.push_str(#key);
								__s.push_str(": ");
								let __v: #ty = ::core::default::Default::default();
								let __y = ::serde_yaml_ng::to_string(&__v).unwrap();
								__s.push_str(__y.trim());
								__s.push_str("  # ");
								__s.push_str(#doc_trim_lit);
								__s.push('\n');
							} else {
								__emit_above_comment(&mut __s, __indent, #doc_lit);
								__s.push_str(&__sp(__indent));
								__s.push_str(#key);
								__s.push_str(": ");
								let __v: #ty = ::core::default::Default::default();
								let __y = ::serde_yaml_ng::to_string(&__v).unwrap();
								__s.push_str(__y.trim());
								__s.push('\n');
							}
						}
					}
				} else {
					// fallback for non-primitive scalar types (should rarely happen)
					quote! {
						#doc_lines
						__s.push_str(&__sp(__indent));
						__s.push_str(#key);
						__s.push_str(": <non-primitive>\n");
					}
				}
			}
		})
		.collect();

	let struct_doc_lit = syn::LitStr::new(&struct_doc, Span::call_site());
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

		impl #name {
			pub fn demo_yaml() -> String {
				Self::demo_yaml_with_indent(0)
			}

			pub(crate) fn demo_yaml_with_indent(__indent: usize) -> String {
				let mut __s = String::new();
				let __sp = |n: usize| -> String { " ".repeat(n) };

				fn __emit_above_comment(buf: &mut String, indent: usize, text: &str) {
					let sp = || " ".repeat(indent);
					for line in text.lines() {
						if line.trim().is_empty() {
							buf.push_str(&sp());
							buf.push_str("#\n");
						} else if line.trim_start().starts_with('#') {
							buf.push_str(&sp());
							buf.push_str("# ");
							buf.push_str(" ");
							buf.push_str(line.trim_start());
							buf.push('\n');
						} else {
							buf.push_str(&sp());
							buf.push_str("# ");
							buf.push_str(line);
							buf.push('\n');
						}
					}
				}

				fn __should_inline_comment(text: &str) -> bool {
					let trimmed = text.trim();
					!trimmed.contains('\n') && trimmed.len() <= 60
				}

				// --- Struct-level doc at top-level
				// Only emit struct-level doc if non-empty, at compile time
				#[allow(unused_braces)]
				{
					const STRUCT_DOC: &str = #struct_doc_lit;
					if !STRUCT_DOC.is_empty() {
						if __indent == 0 {
							__emit_above_comment(&mut __s, __indent, #struct_doc_lit);
							__s.push('\n');
						}
					}
				}

				#( {
					#field_yaml_blocks
					if __indent == 0 {
						__s.push('\n');
					}
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

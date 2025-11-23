//! Proc-macro crate for VersaTiles.
//!
//! This crate provides derive macros and attribute macros to assist with decoding VPL data,
//! generating configuration documentation, and adding error context to functions.
//!
//! # Provided macros
//! - `#[derive(VPLDecode)]`: Derive macro to decode VPL data into Rust structs.
//! - `#[derive(ConfigDoc)]`: Derive macro to generate YAML documentation for configuration structs.
//! - `#[context("...")]`: Attribute macro to add error context to functions returning `Result`.

mod args;
mod config_doc;
mod decode_vpl;

use crate::{args::*, config_doc::*, decode_vpl::*};
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::{ToTokens, quote};
use syn::{Fields, parse_macro_input, spanned::Spanned};

/// Derive macro to decode VPL data into Rust structs.
///
/// This macro can be applied to named-field structs to automatically generate decoding logic
/// from VPL (VersaTiles Programming Language) data.
#[proc_macro_derive(VPLDecode)]
pub fn decode_vpl(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as syn::DeriveInput);

	let expanded = match input.data.clone() {
		syn::Data::Struct(data_struct) => decode_struct(input, data_struct),
		_ => panic!("VPLDecode can only be derived for structs, but: {:?}", input.data),
	};

	TokenStream::from(expanded)
}

/// Derive macro to generate YAML configuration documentation.
///
/// `ConfigDoc` generates a YAML-formatted demo of the configuration struct, including documentation
/// comments and example values. It supports the following attributes:
///
/// - `serde(rename = "...")` to rename keys in the output.
/// - `#[config_demo("...")]` to provide example values for fields.
///
/// Nested structs are rendered recursively, and `Vec<T>` fields are rendered as YAML lists.
///
/// # Example
///
/// Given:
///
/// ```rust
/// use versatiles_derive::ConfigDoc;
///
/// #[derive(ConfigDoc)]
/// struct Config {
///     /// The name of the user.
///     #[config_demo("alice")]
///     name: String,
///
///     /// List of roles.
///     #[config_demo("- admin\n- user")]
///     roles: Vec<String>,
///
///     /// Nested settings.
///     settings: Settings,
/// }
///
/// #[derive(ConfigDoc)]
/// struct Settings {
///     /// Enable feature.
///     #[config_demo("true")]
///     enabled: bool,
/// }
/// ```
///
/// The generated YAML demo might look like:
///
/// ```yaml
/// # The name of the user.
/// username: alice
///
/// # List of roles.
/// roles:
///   - admin
///   - user
///
/// # Nested settings.
/// settings:
///   # Enable feature.
///   enabled: true
/// ```
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
		inner_ty_vec: Option<syn::Type>, // Vec<T> -> T
		// Heuristic: treat non-Option/Vec/Map path types as nested
		is_nested_struct: bool,
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
				if let Some(mut inners) = angle_inner(&f.ty)
					&& let Some(inner) = inners.pop()
				{
					inner_ty_vec = Some(inner.clone());
				}
			} else if id_s == "HashMap" {
				is_map = true;
			}
		}

		let is_url_path = is_url_path(&f.ty);

		// parse demo from #[config_demo("...")] or #[config_demo(value = "...")]
		let mut demo_value = None;
		for attr in &f.attrs {
			if attr.path().is_ident("config_demo") {
				// Prefer positional literal: #[config_demo("...")]
				if demo_value.is_none()
					&& let Ok(lit) = attr.parse_args::<syn::LitStr>()
				{
					demo_value = Some(lit.value());
					continue;
				}
			}
		}

		// decide if nested struct
		let is_nested_struct =
			!is_option && !is_vec && !is_map && path_ident(&f.ty).is_some() && !is_primitive_like(&f.ty) && !is_url_path;

		rows.push(Row {
			ident,
			key,
			ty,
			doc,
			is_vec,
			inner_ty_vec,
			is_nested_struct,
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

			let mut output: TokenStream2 = quote! {
				__s.push_str(&__sp(__indent));
				__s.push('\n');
				for line in #doc_lit.lines() {
					__s.push_str(&__sp(__indent));
					__s.push_str("# ");
					__s.push_str(line);
					__s.push('\n');
				}
				__s.push_str(&__sp(__indent));
				__s.push_str(#key_lit);
				__s.push_str(": ");
			};

			if r.is_nested_struct {
				output = quote! {
					#output
					__s.push_str("\n");
					__s.push_str(&<#ty>::demo_yaml_with_indent(__indent + 2));
				};
			} else if let Some(demo_lit) = &demo_lit {
				output = quote! {
					#output
					__s.push_str(#demo_lit);
				};
			} else if r.is_vec
				&& let Some(inner) = &r.inner_ty_vec
			{
				output = quote! {
					#output
					__s.push_str("\n");
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
			quote! {
				#output
				if !__s.ends_with('\n') {
					__s.push('\n');
				}
			}
		})
		.collect();

	let expanded = quote! {
		impl #name {
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

/// Attribute macro to add error context to functions returning `Result`.
///
/// This macro wraps the function body to attach additional context to errors using `anyhow::Context`.
///
/// It supports:
/// - **Sync functions** returning `Result`: wraps the body and maps errors with context.
/// - **`async fn` functions** returning `Result`: awaits the async block and maps errors with context.
/// - **Functions lowered by `async_trait`** (returning pinned futures): wraps the returned future and maps errors with context.
///
/// # Examples
///
/// Sync function:
///
/// ```rust
/// use versatiles_derive::context;
/// use anyhow::Result;
/// #[context("failed to process data")]
/// fn process() -> Result<()> {
///     // ...
///     Ok(())
/// }
/// ```
///
/// Async function:
///
/// ```rust
/// use versatiles_derive::context;
/// use anyhow::Result;
/// #[context("failed to fetch data")]
/// async fn fetch() -> Result<String> {
///     // ...
///     Ok("data".to_string())
/// }
/// ```
///
/// Function lowered by `async_trait`:
///
/// ```rust
/// use versatiles_derive::context;
/// use anyhow::Result;
/// use std::pin::Pin;
/// #[context("failed in async trait method")]
/// fn async_trait_method() -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
///     // ...
///     Box::pin(async { Ok(()) })
/// }
/// ```
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
		quote! {{
			use ::anyhow::Context as _;
			let #result: #return_type = (async #move_token { #body }).await;
			#result.map_err(|#err| #err.context(format!(#format_args)).into())
		}}
	} else {
		{
			// Heuristic: if the syntactic return type's last path segment is `Pin`, assume `async_trait` lowered fn
			let is_pin_return = matches!(
				&return_type,
				syn::ReturnType::Type(_, ty)
					if matches!(ty.as_ref(),
						syn::Type::Path(tp) if tp.path.segments.last().map(|s| s.ident == "Pin").unwrap_or(false)
					)
			);

			if is_pin_return {
				// async_trait-lowered: return a boxed async block that awaits the inner future and maps the Result
				quote! {{
					use ::anyhow::Context as _;
					let __fut = (|| #return_type { #body })();
					::core::pin::Pin::from(Box::new(async move {
						let __res = __fut.await;
						__res.map_err(|#err| #err.context(format!(#format_args)).into())
					}))
				}}
			} else {
				// Truly sync function returning Result<...>
				let force_fn_once = Ident::new("force_fn_once", Span::mixed_site());
				quote! {{
					use ::anyhow::Context as _;
					let #force_fn_once = ::core::iter::empty::<()>();
					(#move_token || #return_type {
						::core::mem::drop(#force_fn_once);
						#body
					})().map_err(|#err| #err.context(format!(#format_args)).into())
				}}
			}
		}
	};
	input.block.stmts = vec![syn::Stmt::Expr(syn::Expr::Verbatim(new_body), None)];

	input.into_token_stream().into()
}

//! Helper functions for the ConfigDoc derive macro.
//!
//! These utilities extract type information and attributes from syn AST nodes
//! to support YAML documentation generation.

use quote::ToTokens;
use syn::Type;

/// Collects all doc comments from a list of attributes into a single string.
///
/// Each `#[doc = "..."]` attribute is extracted, trimmed, and joined with newlines.
///
/// # Example
///
/// ```ignore
/// /// First line
/// /// Second line
/// struct Foo;
/// ```
///
/// Would produce: `"First line\nSecond line"`
pub fn collect_doc(attrs: &[syn::Attribute]) -> String {
	let mut lines: Vec<String> = Vec::new();
	for attr in attrs {
		if !attr.path().is_ident("doc") {
			continue;
		}
		if let syn::Meta::NameValue(nv) = &attr.meta
			&& let syn::Expr::Lit(expr_lit) = &nv.value
			&& let syn::Lit::Str(lit) = &expr_lit.lit
		{
			let v = lit.value();
			lines.push(v.trim().to_string());
		}
	}
	lines.join("\n")
}

/// Extracts a renamed field name from `#[serde(rename = "...")]` attribute.
///
/// Returns `Some(renamed)` if the attribute exists, `None` otherwise.
///
/// # Example
///
/// ```ignore
/// #[serde(rename = "user_name")]
/// name: String,
/// ```
///
/// Would return `Some("user_name".to_string())`
pub fn serde_rename(attrs: &[syn::Attribute]) -> Option<String> {
	for attr in attrs {
		if attr.path().is_ident("serde") {
			let mut out: Option<String> = None;
			let _ = attr.parse_nested_meta(|meta| {
				if meta.path.is_ident("rename")
					&& let Ok(v) = meta.value()
					&& let Ok(s) = v.parse::<syn::LitStr>()
				{
					out = Some(s.value());
				}
				Ok(())
			});
			if out.is_some() {
				return out;
			}
		}
	}
	None
}

/// Checks if a type is `Option<T>`.
///
/// Returns `true` if the type's last path segment is `Option`.
pub fn is_option(ty: &Type) -> bool {
	if let Type::Path(tp) = ty
		&& let Some(seg) = tp.path.segments.last()
	{
		return seg.ident == "Option";
	}
	false
}

/// Checks if a type should be rendered as a scalar (inline) value in YAML.
///
/// Primitive types like `bool`, integers, floats, `String`, and `&str` are
/// considered "primitive-like" and rendered inline rather than as nested objects.
pub fn is_primitive_like(ty: &syn::Type) -> bool {
	let s = ty.to_token_stream().to_string();
	matches!(
		s.as_str(),
		"bool"
			| "u8" | "u16"
			| "u32"
			| "u64"
			| "u128"
			| "i8" | "i16"
			| "i32"
			| "i64"
			| "i128"
			| "f32"
			| "f64"
			| "String"
			| "& str"
	)
}

/// Extracts the last segment identifier from a type path.
///
/// For `std::vec::Vec<T>`, returns `Some(&Ident("Vec"))`.
/// For non-path types, returns `None`.
pub fn path_ident(ty: &syn::Type) -> Option<&syn::Ident> {
	if let syn::Type::Path(tp) = ty {
		tp.path.segments.last().map(|s| &s.ident)
	} else {
		None
	}
}

/// Extracts the inner types from angle-bracketed generic arguments.
///
/// For `Vec<String>`, returns `Some(vec![String])`.
/// For `HashMap<K, V>`, returns `Some(vec![K, V])`.
/// For non-generic types, returns `None`.
pub fn angle_inner(ty: &syn::Type) -> Option<Vec<syn::Type>> {
	if let syn::Type::Path(tp) = ty
		&& let Some(seg) = tp.path.segments.last()
		&& let syn::PathArguments::AngleBracketed(args) = &seg.arguments
	{
		let mut v = Vec::new();
		for a in &args.args {
			if let syn::GenericArgument::Type(t) = a {
				v.push(t.clone());
			}
		}
		return Some(v);
	}
	None
}

/// Checks if a type is `DataLocation` (used for URL paths in configuration).
///
/// Returns `true` if the type's last path segment is `DataLocation`.
pub fn is_url_path(ty: &syn::Type) -> bool {
	if let syn::Type::Path(tp) = ty
		&& let Some(seg) = tp.path.segments.last()
	{
		return seg.ident == "DataLocation";
	}
	false
}

#[cfg(test)]
mod tests {
	use super::*;
	use syn::parse_quote;

	#[test]
	fn test_collect_doc() {
		let attrs: Vec<syn::Attribute> = vec![
			parse_quote!(#[doc = "First line"]),
			parse_quote!(#[doc = "Second line"]),
		];
		assert_eq!(collect_doc(&attrs), "First line\nSecond line");
	}

	#[test]
	fn test_collect_doc_empty() {
		let attrs: Vec<syn::Attribute> = vec![];
		assert_eq!(collect_doc(&attrs), "");
	}

	#[test]
	fn test_collect_doc_ignores_non_doc() {
		let attrs: Vec<syn::Attribute> = vec![parse_quote!(#[derive(Debug)]), parse_quote!(#[doc = "Only doc"])];
		assert_eq!(collect_doc(&attrs), "Only doc");
	}

	#[test]
	fn test_serde_rename() {
		let attrs: Vec<syn::Attribute> = vec![parse_quote!(#[serde(rename = "new_name")])];
		assert_eq!(serde_rename(&attrs), Some("new_name".to_string()));
	}

	#[test]
	fn test_serde_rename_none() {
		let attrs: Vec<syn::Attribute> = vec![parse_quote!(#[doc = "no rename here"])];
		assert_eq!(serde_rename(&attrs), None);
	}

	#[test]
	fn test_is_option() {
		let ty: syn::Type = parse_quote!(Option<String>);
		assert!(is_option(&ty));

		let ty: syn::Type = parse_quote!(String);
		assert!(!is_option(&ty));

		let ty: syn::Type = parse_quote!(Vec<u8>);
		assert!(!is_option(&ty));
	}

	#[test]
	fn test_is_primitive_like() {
		let primitives = ["bool", "u8", "u16", "u32", "u64", "i32", "f32", "f64", "String"];
		for p in primitives {
			let ty: syn::Type = syn::parse_str(p).unwrap();
			assert!(is_primitive_like(&ty), "Expected {p} to be primitive-like");
		}

		let non_primitives = ["Vec<u8>", "Option<String>", "MyStruct"];
		for p in non_primitives {
			let ty: syn::Type = syn::parse_str(p).unwrap();
			assert!(!is_primitive_like(&ty), "Expected {p} to NOT be primitive-like");
		}
	}

	#[test]
	fn test_path_ident() {
		use std::string::ToString;

		let ty: syn::Type = parse_quote!(Vec<String>);
		assert_eq!(path_ident(&ty).map(ToString::to_string), Some("Vec".to_string()));

		let ty: syn::Type = parse_quote!(std::vec::Vec<u8>);
		assert_eq!(path_ident(&ty).map(ToString::to_string), Some("Vec".to_string()));

		let ty: syn::Type = parse_quote!(String);
		assert_eq!(path_ident(&ty).map(ToString::to_string), Some("String".to_string()));
	}

	#[test]
	fn test_angle_inner() {
		let ty: syn::Type = parse_quote!(Vec<String>);
		let inner = angle_inner(&ty).unwrap();
		assert_eq!(inner.len(), 1);

		let ty: syn::Type = parse_quote!(HashMap<String, u32>);
		let inner = angle_inner(&ty).unwrap();
		assert_eq!(inner.len(), 2);

		let ty: syn::Type = parse_quote!(String);
		assert!(angle_inner(&ty).is_none());
	}

	#[test]
	fn test_is_url_path() {
		let ty: syn::Type = parse_quote!(DataLocation);
		assert!(is_url_path(&ty));

		let ty: syn::Type = parse_quote!(String);
		assert!(!is_url_path(&ty));
	}
}

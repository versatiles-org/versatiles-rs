#![allow(dead_code, unused_variables)]

use syn::Type;

pub fn collect_doc(attrs: &[syn::Attribute]) -> String {
	let mut lines: Vec<String> = Vec::new();
	for attr in attrs {
		if !attr.path().is_ident("doc") {
			continue;
		}
		match &attr.meta {
			syn::Meta::NameValue(nv) => {
				if let syn::Expr::Lit(expr_lit) = &nv.value {
					if let syn::Lit::Str(lit) = &expr_lit.lit {
						let v = lit.value();
						lines.push(v.trim().to_string());
					}
				}
			}
			_ => {}
		}
	}
	lines.join("\n")
}

pub fn serde_rename(attrs: &[syn::Attribute]) -> Option<String> {
	for attr in attrs {
		if attr.path().is_ident("serde") {
			let mut out: Option<String> = None;
			let _ = attr.parse_nested_meta(|meta| {
				if meta.path.is_ident("rename") {
					if let Ok(v) = meta.value() {
						if let Ok(s) = v.parse::<syn::LitStr>() {
							out = Some(s.value());
						}
					}
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

pub fn is_option(ty: &Type) -> bool {
	if let Type::Path(tp) = ty {
		if let Some(seg) = tp.path.segments.last() {
			return seg.ident == "Option";
		}
	}
	false
}

/// Returns true if the type is a Rust primitive scalar-like config type:
/// bool, integer, float, String, or &str.
pub fn is_primitive_like(ty: &Type) -> bool {
	match ty {
		Type::Path(tp) => {
			if let Some(ident) = path_ident(ty) {
				// Known primitive types
				match ident.to_string().as_str() {
					"bool" | "String" | "u8" | "u16" | "u32" | "u64" | "u128" | "i8" | "i16" | "i32" | "i64" | "i128"
					| "usize" | "isize" | "f32" | "f64" => true,
					_ => false,
				}
			} else {
				false
			}
		}
		Type::Reference(r) => {
			// Check for &str
			if let Type::Path(tp) = &*r.elem {
				if let Some(ident) = path_ident(&Type::Path(tp.clone())) {
					return ident == "str";
				}
			}
			false
		}
		_ => false,
	}
}

/// Returns the last path segment ident if the type is a path, else None.
pub fn path_ident(ty: &Type) -> Option<&syn::Ident> {
	if let Type::Path(tp) = ty {
		tp.path.segments.last().map(|seg| &seg.ident)
	} else {
		None
	}
}

/// Extracts generic inner types from angle-bracketed args (e.g., Vec<T>, Option<T>, HashMap<K,V>).
pub fn angle_inner(ty: &Type) -> Option<Vec<Type>> {
	if let Type::Path(tp) = ty {
		if let Some(seg) = tp.path.segments.last() {
			if let syn::PathArguments::AngleBracketed(ref ab) = seg.arguments {
				let mut out = Vec::new();
				for arg in &ab.args {
					if let syn::GenericArgument::Type(t) = arg {
						out.push(t.clone());
					}
				}
				return Some(out);
			}
		}
	}
	None
}

/// Detects #[serde(flatten)] on a field.
pub fn has_serde_flatten(attrs: &[syn::Attribute]) -> bool {
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

/// Detects the domain type UrlPath by last segment ident.
pub fn is_url_path(ty: &Type) -> bool {
	path_ident(ty).map_or(false, |ident| ident == "UrlPath")
}

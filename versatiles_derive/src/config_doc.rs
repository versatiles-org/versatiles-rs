use quote::ToTokens;
use syn::Type;

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

pub fn is_option(ty: &Type) -> bool {
	if let Type::Path(tp) = ty
		&& let Some(seg) = tp.path.segments.last()
	{
		return seg.ident == "Option";
	}
	false
}

// crude primitive-ish detection for deciding nested vs scalar
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

pub fn path_ident(ty: &syn::Type) -> Option<&syn::Ident> {
	if let syn::Type::Path(tp) = ty {
		tp.path.segments.last().map(|s| &s.ident)
	} else {
		None
	}
}

pub fn angle_inner(ty: &syn::Type) -> Option<Vec<syn::Type>> {
	if let syn::Type::Path(tp) = ty
		&& let Some(seg) = tp.path.segments.last()
		&& let syn::PathArguments::AngleBracketed(args) = &seg.arguments
	{
		let mut v = Vec::new();
		for a in args.args.iter() {
			if let syn::GenericArgument::Type(t) = a {
				v.push(t.clone());
			}
		}
		return Some(v);
	}
	None
}

pub fn is_url_path(ty: &syn::Type) -> bool {
	if let syn::Type::Path(tp) = ty
		&& let Some(seg) = tp.path.segments.last()
	{
		return seg.ident == "UrlPath";
	}
	false
}

use syn::Type;

pub fn collect_doc(attrs: &[syn::Attribute]) -> String {
	let mut lines: Vec<String> = Vec::new();
	for attr in attrs {
		if attr.path().is_ident("doc") {
			// syn v2: parse as NameValue with a string literal
			let _ = attr.parse_nested_meta(|meta| {
				if let Ok(v) = meta.value() {
					if let Ok(lit) = v.parse::<syn::LitStr>() {
						lines.push(lit.value().trim().to_string());
					}
				}
				Ok(())
			});
		}
	}
	lines.join(" ")
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

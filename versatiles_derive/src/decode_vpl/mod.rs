mod decode_struct;

pub use decode_struct::decode_struct;
use syn::{Attribute, Meta};

pub fn extract_comment(attr: &Attribute) -> Option<String> {
	if attr.path().is_ident("doc") {
		if let Meta::NameValue(meta) = &attr.meta {
			if let syn::Expr::Lit(lit) = &meta.value {
				if let syn::Lit::Str(lit_str) = &lit.lit {
					let text = lit_str.value().trim().to_string();
					return if text.is_empty() { None } else { Some(text) };
				}
			}
		}
	}
	None
}

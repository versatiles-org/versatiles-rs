mod definition;
mod kw;
mod node;
mod scalar;
mod variants;

pub use definition::Definition;
use proc_macro2::TokenStream;

fn emit_decoder(def: &Definition) -> syn::Result<TokenStream> {
	match def {
		Definition::Struct(s) => node::emit_struct(s, true),
		Definition::NewType(s) => node::emit_new_type(s),
		Definition::TupleStruct(s) => node::emit_struct(s, false),
		Definition::UnitStruct(s) => node::emit_struct(s, true),
		Definition::Enum(e) => variants::emit_enum(e),
	}
}

pub fn decode_derive(input: &Definition) -> proc_macro::TokenStream {
	match emit_decoder(input) {
		Ok(stream) => stream.into(),
		Err(e) => e.to_compile_error().into(),
	}
}

mod decode_vdl;

use decode_vdl::decode_struct;
use proc_macro::TokenStream;
use syn::{parse_macro_input, Data, DeriveInput};

#[proc_macro_derive(VDLDecode)]
pub fn decode_vdl(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as DeriveInput);

	let expanded = match input.data.clone() {
		Data::Struct(data_struct) => decode_struct(input, data_struct),
		_ => panic!(
			"VDLDecode can only be derived for structs, but: {:?}",
			input.data
		),
	};

	TokenStream::from(expanded)
}

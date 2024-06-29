mod decode_vpl;

use decode_vpl::decode_struct;
use proc_macro::TokenStream;
use syn::{parse_macro_input, Data, DeriveInput};

#[proc_macro_derive(VPLDecode)]
pub fn decode_vpl(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as DeriveInput);

	let expanded = match input.data.clone() {
		Data::Struct(data_struct) => decode_struct(input, data_struct),
		_ => panic!(
			"VPLDecode can only be derived for structs, but: {:?}",
			input.data
		),
	};

	TokenStream::from(expanded)
}

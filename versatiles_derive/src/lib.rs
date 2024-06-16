mod decode_kdl;

use decode_kdl::{decode_enum, decode_struct};
use proc_macro::TokenStream;
use syn::{parse_macro_input, Data, DeriveInput};

#[proc_macro_derive(KDLDecode, attributes(kdl))]
pub fn decode_kdl(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as DeriveInput);

	let expanded = match input.data.clone() {
		Data::Struct(data_struct) => decode_struct(input, data_struct),
		Data::Enum(data_enum) => decode_enum(input, data_enum),
		_ => panic!(
			"KDLDecode can only be derived for structs and enums {:?}",
			input.data
		),
	};

	TokenStream::from(expanded)
}

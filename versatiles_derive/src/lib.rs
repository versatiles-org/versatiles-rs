//mod doc;
mod kdl;

extern crate proc_macro;
extern crate quote;
extern crate syn;

use kdl::Definition;
use proc_macro::TokenStream;
use syn::parse_macro_input;

#[proc_macro_derive(KDLDecode, attributes(kdl))]
pub fn kdl_decode(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as Definition);
	//let doc = doc::get_doc(&input);
	let kdl = kdl::decode_derive(&input);

	TokenStream::from(kdl)
}

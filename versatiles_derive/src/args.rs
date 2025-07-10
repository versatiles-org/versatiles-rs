extern crate proc_macro;

use proc_macro2::TokenStream as TokenStream2;
use syn::Token;
use syn::parse::{self, Parse, ParseStream};

pub struct Args(pub Option<Token![move]>, pub TokenStream2);
impl Parse for Args {
	fn parse(input: ParseStream<'_>) -> parse::Result<Self> {
		let move_token = if input.peek(Token![move]) {
			let token = input.parse()?;
			input.parse::<Token![,]>()?;
			Some(token)
		} else {
			None
		};
		Ok(Self(move_token, input.parse()?))
	}
}

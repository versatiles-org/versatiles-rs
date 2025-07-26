extern crate proc_macro;

use proc_macro2::TokenStream as TokenStream2;
use syn::Token;
use syn::parse::{self, Parse, ParseStream};

#[derive(Debug)]
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

#[cfg(test)]
mod tests {
	use super::Args;
	use syn::parse_str;

	#[test]
	fn test_parse_without_move() {
		let args: Args = parse_str("foo").expect("Failed to parse without move");
		assert!(args.0.is_none());
		assert_eq!(args.1.to_string(), "foo");
	}

	#[test]
	fn test_parse_with_move() {
		let args: Args = parse_str("move, foo").expect("Failed to parse with move");
		assert!(args.0.is_some());
		assert_eq!(args.1.to_string(), "foo");
	}

	#[test]
	fn test_parse_complex_expression() {
		let input = "move, a + b * c";
		let args: Args = parse_str(input).expect("Failed to parse complex expression");
		assert!(args.0.is_some());
		assert_eq!(args.1.to_string(), "a + b * c");
	}

	#[test]
	fn test_parse_invalid_missing_comma() {
		let err = parse_str::<Args>("move foo").unwrap_err();
		assert!(err.to_string().contains(","), "Expected comma error, got: {}", err);
	}
}

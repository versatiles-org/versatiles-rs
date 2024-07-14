use super::iterator::CharIterator;
use anyhow::{ensure, Result};

pub fn parse_tag(iter: &mut CharIterator, text: &str) -> Result<()> {
	for c in text.chars() {
		match iter.get_next_char()? {
			b if b == c => continue,
			_ => return Err(iter.build_error("unexpected character while parsing 'null'")),
		}
	}
	Ok(())
}

pub fn parse_string(iter: &mut CharIterator) -> Result<String> {
	ensure!(iter.get_next_char()? == '"');

	let mut string = String::new();
	loop {
		match iter.get_next_char()? {
			'"' => break,
			'\\' => match iter.get_next_char()? {
				'"' => string.push('"'),
				'\\' => string.push('\\'),
				'/' => string.push('/'),
				'b' => string.push('\x08'),
				'f' => string.push('\x0C'),
				'n' => string.push('\n'),
				'r' => string.push('\r'),
				't' => string.push('\t'),
				'u' => {
					let mut hex = String::new();
					for _ in 0..4 {
						hex.push(iter.get_next_char()?);
					}
					let code_point = u16::from_str_radix(&hex, 16)
						.map_err(|_| iter.build_error("invalid unicode code point"))?;
					string.push(
						char::from_u32(code_point as u32)
							.ok_or_else(|| iter.build_error("invalid unicode code point"))?,
					);
				}
				c => string.push(c),
			},
			c => string.push(c),
		}
	}
	Ok(string)
}

pub fn parse_number_as_f64(iter: &mut CharIterator) -> Result<f64> {
	let mut number = String::new();
	while let Some(c) = iter.peek_char() {
		if c.is_ascii_digit() || *c == '-' || *c == '.' {
			number.push(*c);
			iter.skip_char();
		} else {
			break;
		}
	}
	number
		.parse::<f64>()
		.map_err(|_| iter.build_error("invalid number"))
}

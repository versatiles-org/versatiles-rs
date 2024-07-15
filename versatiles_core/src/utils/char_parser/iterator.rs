use anyhow::{anyhow, Error, Result};

const RING_SIZE: usize = 16;

pub struct CharIterator<'a> {
	iter: Box<dyn Iterator<Item = char> + Send + 'a>,
	next_char: Option<char>,
	char_pos: usize,
	byte_pos: usize,
	debug: bool,
	ring: Vec<String>,
}

#[allow(dead_code)]
impl<'a> CharIterator<'a> {
	pub fn new(chars: impl Iterator<Item = char> + Send + 'a, debug: bool) -> Result<Self> {
		let mut me = CharIterator {
			iter: Box::new(chars),
			next_char: None,
			char_pos: 0,
			byte_pos: 0,
			debug,
			ring: Vec::new(),
		};
		me.skip_char();
		Ok(me)
	}

	pub fn build_error(&self, msg: &str) -> Error {
		if self.debug {
			let mut ring = String::new();
			for i in 0..RING_SIZE {
				let index = (self.char_pos + i) % RING_SIZE;
				ring.push_str(self.ring.get(index).unwrap_or(&String::new()));
			}
			anyhow!("{msg} at pos {}: {}", self.char_pos, ring)
		} else {
			anyhow!("{msg} at pos {}", self.char_pos)
		}
	}

	pub fn byte_pos(&self) -> usize {
		self.byte_pos
	}

	pub fn peek_char(&self) -> &Option<char> {
		&self.next_char
	}

	pub fn skip_char(&mut self) {
		self.next_char = self.iter.next();
		if self.debug {
			let char = if let Some(c) = self.next_char {
				c.to_string()
			} else {
				String::from("<EOF>")
			};
			let index = self.char_pos % RING_SIZE;
			if self.ring.len() <= index {
				self.ring.push(char);
			} else {
				self.ring[index] = char;
			}
		}
		self.char_pos += 1;
		if let Some(c) = self.next_char {
			self.byte_pos += c.len_utf8();
		}
	}

	pub fn next_char(&mut self) -> Option<char> {
		let next_char = self.next_char;
		self.skip_char();
		next_char
	}

	pub fn get_next_char(&mut self) -> Result<char> {
		self
			.next_char()
			.ok_or_else(|| self.build_error("unexpected end"))
	}

	pub fn get_peek_char(&mut self) -> Result<char> {
		self
			.peek_char()
			.ok_or_else(|| self.build_error("unexpected end"))
	}

	pub fn skip_whitespace(&mut self) -> Result<()> {
		while let Some(b) = self.peek_char() {
			if !b.is_ascii_whitespace() {
				break;
			}
			self.next_char();
		}
		Ok(())
	}

	pub fn into_string(mut self) -> String {
		std::iter::from_fn(move || self.next_char()).collect()
	}
}

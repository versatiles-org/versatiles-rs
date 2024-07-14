use anyhow::{anyhow, Error, Result};
use std::str::{self, Chars};

const RING_SIZE: usize = 16;

pub struct CharIterator<'a> {
	iter: Chars<'a>,
	next_char: Option<char>,
	pos: u64,
	debug: bool,
	ring: Vec<String>,
}

#[allow(dead_code)]
impl<'a> CharIterator<'a> {
	pub fn new(inner_chars: Chars<'a>, debug: bool) -> Result<Self> {
		let mut me = CharIterator {
			iter: inner_chars,
			next_char: None,
			pos: 0,
			debug,
			ring: Vec::new(),
		};
		me.skip_char();
		Ok(me)
	}

	pub fn build_error(&self, msg: &str) -> Error {
		if self.debug {
			let mut ring = String::new();
			for i in 0..RING_SIZE as u64 {
				let index = (self.pos + i) % RING_SIZE as u64;
				ring.push_str(self.ring.get(index as usize).unwrap_or(&String::new()));
			}
			anyhow!("{msg} at pos {}: {}", self.pos, ring)
		} else {
			anyhow!("{msg} at pos {}", self.pos)
		}
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
			let index = self.pos as usize % RING_SIZE;
			if self.ring.len() <= index {
				self.ring.push(char);
			} else {
				self.ring[index] = char;
			}
		}
		self.pos += 1;
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
}

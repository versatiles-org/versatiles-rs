use anyhow::{bail, ensure, Result};
use std::{
	collections::BTreeMap,
	str::{self, Chars},
};

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
		me.skip();
		Ok(me)
	}

	pub fn error(&self, msg: &str) -> Result<()> {
		if self.debug {
			let mut ring = String::new();
			for i in 0..RING_SIZE as u64 {
				let index = (self.pos + i) % RING_SIZE as u64;
				ring.push_str(self.ring.get(index as usize).unwrap_or(&String::new()));
			}
			bail!("{msg} at pos {}: {}", self.pos, ring);
		} else {
			bail!("{msg} at pos {}", self.pos);
		}
	}

	pub fn peek(&self) -> &Option<char> {
		&self.next_char
	}

	pub fn skip(&mut self) -> () {
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

	pub fn next(&mut self) -> Option<char> {
		let next_char = self.next_char;
		self.skip();
		next_char
	}

	pub fn get_next(&mut self) -> Result<char> {
		self
			.next()
			.ok_or_else(|| self.error("unexpected end of file").unwrap_err())
	}

	pub fn get_peek(&mut self) -> Result<char> {
		self
			.peek()
			.ok_or_else(|| self.error("unexpected end of file").unwrap_err())
	}

	pub fn skip_whitespace(&mut self) -> Result<()> {
		while let Some(b) = self.peek() {
			if !b.is_ascii_whitespace() {
				break;
			}
			self.next();
		}
		Ok(())
	}
}

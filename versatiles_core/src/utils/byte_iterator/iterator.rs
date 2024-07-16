use std::io::Read;

use anyhow::{anyhow, Error, Result};

const RING_SIZE: usize = 16;

pub struct ByteIterator<'a> {
	iter: Box<dyn Iterator<Item = u8> + Send + 'a>,
	next_byte: Option<u8>,
	byte_pos: usize,
	debug: bool,
	ring: Vec<u8>,
}

#[allow(dead_code)]
impl<'a> ByteIterator<'a> {
	pub fn new(bytes: impl Iterator<Item = u8> + Send + 'a, debug: bool) -> Result<Self> {
		let mut me = ByteIterator {
			iter: Box::new(bytes),
			next_byte: None,
			byte_pos: 0,
			debug,
			ring: Vec::new(),
		};
		me.skip_byte();
		Ok(me)
	}

	pub fn from_reader(reader: impl Read + Send + 'a, debug: bool) -> Result<Self> {
		ByteIterator::new(reader.bytes().map(|e| e.unwrap()), debug)
	}

	pub fn build_error(&self, msg: &str) -> Error {
		if self.debug {
			let mut ring = Vec::new();
			for i in 0..RING_SIZE {
				let index = (self.byte_pos + i) % RING_SIZE;
				if let Some(v) = self.ring.get(index) {
					ring.push(*v)
				}
			}
			let mut ring = String::from_utf8(ring).unwrap();
			if self.next_byte.is_none() {
				ring.push_str("<EOF>");
			}
			anyhow!("{msg} at pos {}: {}", self.byte_pos, ring)
		} else {
			anyhow!("{msg} at pos {}", self.byte_pos)
		}
	}

	pub fn byte_pos(&self) -> usize {
		self.byte_pos
	}

	pub fn peek_byte(&self) -> &Option<u8> {
		&self.next_byte
	}

	pub fn skip_byte(&mut self) {
		self.next_byte = self.iter.next();
		if self.debug {
			if let Some(byte) = self.next_byte {
				let index = self.byte_pos % RING_SIZE;
				if self.ring.len() <= index {
					self.ring.push(byte);
				} else {
					self.ring[index] = byte;
				}
			}
		}
		self.byte_pos += 1;
	}

	pub fn next_byte(&mut self) -> Option<u8> {
		let next_byte = self.next_byte;
		self.skip_byte();
		next_byte
	}

	pub fn get_next_byte(&mut self) -> Result<u8> {
		self
			.next_byte()
			.ok_or_else(|| self.build_error("unexpected end"))
	}

	pub fn get_peek_byte(&mut self) -> Result<u8> {
		self
			.peek_byte()
			.ok_or_else(|| self.build_error("unexpected end"))
	}

	pub fn skip_whitespace(&mut self) -> Result<()> {
		while let Some(b) = self.peek_byte() {
			if !b.is_ascii_whitespace() {
				break;
			}
			self.next_byte();
		}
		Ok(())
	}

	pub fn into_string(mut self) -> String {
		String::from_utf8(std::iter::from_fn(move || self.next_byte()).collect()).unwrap()
	}
}

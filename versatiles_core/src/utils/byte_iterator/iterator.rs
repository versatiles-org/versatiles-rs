use anyhow::{anyhow, Error, Result};
use std::io::Read;

const DEBUG_RING_BUFFER_SIZE: usize = 16;

pub struct ByteIterator<'a> {
	iter: Box<dyn Iterator<Item = u8> + Send + 'a>,
	peeked_byte: Option<u8>,
	position: usize,
	is_debug_enabled: bool,
	debug_buffer: Vec<u8>,
}

impl<'a> ByteIterator<'a> {
	pub fn from_iterator(bytes: impl Iterator<Item = u8> + Send + 'a, debug: bool) -> Self {
		let mut instance = ByteIterator {
			iter: Box::new(bytes),
			peeked_byte: None,
			position: 0,
			is_debug_enabled: debug,
			debug_buffer: Vec::new(),
		};
		instance.advance();
		instance
	}

	pub fn from_reader(reader: impl Read + Send + 'a, debug: bool) -> Self {
		ByteIterator::from_iterator(reader.bytes().map(|b| b.unwrap()), debug)
	}

	pub fn format_error(&self, msg: &str) -> Error {
		if self.is_debug_enabled {
			let mut ring = Vec::new();
			for i in 0..DEBUG_RING_BUFFER_SIZE {
				let index = (self.position + i) % DEBUG_RING_BUFFER_SIZE;
				if let Some(&value) = self.debug_buffer.get(index) {
					ring.push(value);
				}
			}
			let mut debug_output = String::from_utf8(ring).unwrap();
			if self.peeked_byte.is_none() {
				debug_output.push_str("<EOF>");
			}
			anyhow!("{msg} at position {}: {}", self.position, debug_output)
		} else {
			anyhow!("{msg} at position {}", self.position)
		}
	}

	pub fn position(&self) -> usize {
		self.position
	}

	pub fn peek(&self) -> &Option<u8> {
		&self.peeked_byte
	}

	pub fn advance(&mut self) {
		self.peeked_byte = self.iter.next();
		if self.is_debug_enabled {
			if let Some(byte) = self.peeked_byte {
				let index = self.position % DEBUG_RING_BUFFER_SIZE;
				if self.debug_buffer.len() <= index {
					self.debug_buffer.push(byte);
				} else {
					self.debug_buffer[index] = byte;
				}
			}
		}
		self.position += 1;
	}

	pub fn consume(&mut self) -> Option<u8> {
		let current_byte = self.peeked_byte;
		self.advance();
		current_byte
	}

	pub fn expect_next_byte(&mut self) -> Result<u8> {
		self
			.consume()
			.ok_or_else(|| self.format_error("unexpected end"))
	}

	pub fn expect_peeked_byte(&self) -> Result<u8> {
		self
			.peek()
			.ok_or_else(|| self.format_error("unexpected end"))
	}

	pub fn skip_whitespace(&mut self) -> Result<()> {
		while let Some(byte) = self.peek() {
			if !byte.is_ascii_whitespace() {
				break;
			}
			self.consume();
		}
		Ok(())
	}

	pub fn into_string(mut self) -> String {
		String::from_utf8(std::iter::from_fn(move || self.consume()).collect()).unwrap()
	}
}

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

	pub fn skip_whitespace(&mut self) {
		while let Some(byte) = self.peek() {
			if !byte.is_ascii_whitespace() {
				break;
			}
			self.consume();
		}
	}

	pub fn into_string(mut self) -> String {
		String::from_utf8(std::iter::from_fn(move || self.consume()).collect()).unwrap()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Cursor;

	#[test]
	fn test_from_iterator() {
		let data = vec![b'a', b'b', b'c'];
		let mut b = ByteIterator::from_iterator(data.into_iter(), false);

		assert_eq!(b.consume(), Some(b'a'));
		assert_eq!(b.consume(), Some(b'b'));
		assert_eq!(b.consume(), Some(b'c'));
		assert_eq!(b.consume(), None);
	}

	#[test]
	fn test_from_reader() {
		let data = Cursor::new(vec![b'x', b'y', b'z']);
		let mut b = ByteIterator::from_reader(data, false);

		assert_eq!(b.consume(), Some(b'x'));
		assert_eq!(b.consume(), Some(b'y'));
		assert_eq!(b.consume(), Some(b'z'));
		assert_eq!(b.consume(), None);
	}

	#[test]
	fn test_peek_and_consume() {
		let data = vec![b'1', b'2', b'3'];
		let mut b = ByteIterator::from_iterator(data.into_iter(), false);

		assert_eq!(b.peek(), &Some(b'1'));
		assert_eq!(b.consume(), Some(b'1'));
		assert_eq!(b.peek(), &Some(b'2'));
		assert_eq!(b.consume(), Some(b'2'));
		assert_eq!(b.consume(), Some(b'3'));
		assert_eq!(b.peek(), &None);
	}

	#[test]
	fn test_expect_next_byte() {
		let data = vec![b'A', b'B'];
		let mut b = ByteIterator::from_iterator(data.into_iter(), false);

		assert_eq!(b.expect_next_byte().unwrap(), b'A');
		assert_eq!(b.expect_next_byte().unwrap(), b'B');
		assert!(b.expect_next_byte().is_err());
	}

	#[test]
	fn test_expect_peeked_byte() {
		let data = vec![b'X', b'Y'];
		let mut b = ByteIterator::from_iterator(data.into_iter(), false);

		assert_eq!(b.expect_peeked_byte().unwrap(), b'X');
		b.consume();
		assert_eq!(b.expect_peeked_byte().unwrap(), b'Y');
		b.consume();
		assert!(b.expect_peeked_byte().is_err());
	}

	#[test]
	fn test_skip_whitespace() {
		let data = vec![b' ', b'\t', b'\n', b'A', b'B'];
		let mut b = ByteIterator::from_iterator(data.into_iter(), false);

		b.skip_whitespace();
		assert_eq!(b.consume(), Some(b'A'));
		assert_eq!(b.consume(), Some(b'B'));
	}

	#[test]
	fn test_into_string() {
		let data = vec![b'H', b'e', b'l', b'l', b'o'];
		let b = ByteIterator::from_iterator(data.into_iter(), false);

		assert_eq!(b.into_string(), "Hello");
	}

	#[test]
	fn test_debug_error_formatting() {
		let data = vec![b'R', b'u', b's', b't'];
		let mut b = ByteIterator::from_iterator(data.into_iter(), true);

		b.consume(); // R
		b.consume(); // u
		b.consume(); // s
		let error = b.format_error("Testing error");

		assert!(format!("{}", error).contains("Testing error at position"));
	}

	#[test]
	fn test_debug_ring_buffer() {
		let data = vec![b'a'; DEBUG_RING_BUFFER_SIZE + 5];
		let mut b = ByteIterator::from_iterator(data.into_iter(), true);

		for _ in 0..DEBUG_RING_BUFFER_SIZE + 5 {
			b.consume();
		}

		assert_eq!(b.debug_buffer.len(), DEBUG_RING_BUFFER_SIZE);
	}
}

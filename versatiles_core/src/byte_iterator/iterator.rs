use anyhow::{Error, Result, anyhow};
use std::io::Read;

const DEBUG_RING_BUFFER_SIZE: usize = 16;
const BUFFER_SIZE: usize = 4096;

pub struct ByteIterator<'a> {
	buffer: [u8; BUFFER_SIZE],
	buffer_len: usize,
	buffer_pos: usize,
	source: Box<dyn Read + 'a>,
	peeked_byte: Option<u8>,
	position: usize,
	is_debug_enabled: bool,
	debug_buffer: [u8; DEBUG_RING_BUFFER_SIZE],
}

impl<'a> ByteIterator<'a> {
	pub fn from_reader(reader: impl Read + 'a, debug: bool) -> Self {
		let mut instance = ByteIterator {
			buffer: [0; BUFFER_SIZE],
			buffer_len: 0,
			buffer_pos: 0,
			source: Box::new(reader),
			peeked_byte: None,
			position: 0,
			is_debug_enabled: debug,
			debug_buffer: [0; DEBUG_RING_BUFFER_SIZE],
		};
		instance.fill_buffer();
		instance.advance();
		instance
	}

	#[inline]
	fn fill_buffer(&mut self) {
		self.buffer_len = self.source.read(&mut self.buffer).unwrap_or(0);
		self.buffer_pos = 0;
	}

	#[inline]
	fn next_byte(&mut self) -> Option<u8> {
		if self.buffer_pos >= self.buffer_len {
			self.fill_buffer();
			if self.buffer_len == 0 {
				return None;
			}
		}
		let byte = self.buffer[self.buffer_pos];
		self.buffer_pos += 1;
		Some(byte)
	}

	#[must_use] 
	pub fn format_error(&self, msg: &str) -> Error {
		if self.is_debug_enabled {
			let (start_index, length) = if self.position < DEBUG_RING_BUFFER_SIZE {
				(0, self.position - 1)
			} else {
				(self.position % DEBUG_RING_BUFFER_SIZE, DEBUG_RING_BUFFER_SIZE - 1)
			};

			let debug_snapshot: Vec<u8> = self
				.debug_buffer
				.iter()
				.cycle()
				.skip(start_index)
				.take(length)
				.copied()
				.collect();

			let mut debug_output = String::from_utf8(debug_snapshot).unwrap();
			if self.peeked_byte.is_none() {
				debug_output.push_str("<EOF>");
			}
			anyhow!("{msg} at position {}: {}", self.position - 1, debug_output)
		} else {
			anyhow!("{msg} at position {}", self.position - 1)
		}
	}

	#[inline]
	#[must_use] 
	pub fn position(&self) -> usize {
		self.position
	}

	#[inline]
	#[must_use] 
	pub fn peek(&self) -> Option<u8> {
		self.peeked_byte
	}

	#[inline]
	pub fn advance(&mut self) {
		self.peeked_byte = self.next_byte();
		if self.is_debug_enabled
			&& let Some(byte) = self.peeked_byte
		{
			let index = self.position % DEBUG_RING_BUFFER_SIZE;
			self.debug_buffer[index] = byte;
		}
		self.position += 1;
	}

	#[inline]
	pub fn consume(&mut self) -> Option<u8> {
		let current_byte = self.peeked_byte;
		self.advance();
		current_byte
	}

	#[inline]
	pub fn expect_next_byte(&mut self) -> Result<u8> {
		if let Some(current_byte) = self.peeked_byte {
			self.advance();
			Ok(current_byte)
		} else {
			Err(self.format_error("unexpected end"))
		}
	}

	#[inline]
	pub fn expect_peeked_byte(&self) -> Result<u8> {
		self.peeked_byte.ok_or_else(|| self.format_error("unexpected end"))
	}

	#[inline]
	pub fn skip_whitespace(&mut self) {
		while let Some(byte) = self.peek() {
			if !byte.is_ascii_whitespace() {
				break;
			}
			self.advance();
		}
	}

	pub fn into_string(mut self) -> Result<String> {
		let mut result = Vec::new();
		while let Some(byte) = self.consume() {
			result.push(byte);
		}
		String::from_utf8(result).map_err(anyhow::Error::from)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Cursor;

	#[test]
	fn test_from_iterator() {
		let reader = Cursor::new(vec![b'a', b'b', b'c']);
		let mut b = ByteIterator::from_reader(reader, false);

		assert_eq!(b.consume(), Some(b'a'));
		assert_eq!(b.consume(), Some(b'b'));
		assert_eq!(b.consume(), Some(b'c'));
		assert_eq!(b.consume(), None);
	}

	#[test]
	fn test_from_reader() {
		let reader = Cursor::new(vec![b'x', b'y', b'z']);
		let mut b = ByteIterator::from_reader(reader, false);

		assert_eq!(b.consume(), Some(b'x'));
		assert_eq!(b.consume(), Some(b'y'));
		assert_eq!(b.consume(), Some(b'z'));
		assert_eq!(b.consume(), None);
	}

	#[test]
	fn test_peek_and_consume() {
		let reader = Cursor::new(vec![b'1', b'2', b'3']);
		let mut b = ByteIterator::from_reader(reader, false);

		assert_eq!(b.peek(), Some(b'1'));
		assert_eq!(b.consume(), Some(b'1'));
		assert_eq!(b.peek(), Some(b'2'));
		assert_eq!(b.consume(), Some(b'2'));
		assert_eq!(b.consume(), Some(b'3'));
		assert_eq!(b.peek(), None);
	}

	#[test]
	fn test_expect_next_byte() {
		let reader = Cursor::new(vec![b'A', b'B']);
		let mut b = ByteIterator::from_reader(reader, false);

		assert_eq!(b.expect_next_byte().unwrap(), b'A');
		assert_eq!(b.expect_next_byte().unwrap(), b'B');
		assert!(b.expect_next_byte().is_err());
	}

	#[test]
	fn test_expect_peeked_byte() {
		let reader = Cursor::new(vec![b'X', b'Y']);
		let mut b = ByteIterator::from_reader(reader, false);

		assert_eq!(b.expect_peeked_byte().unwrap(), b'X');
		b.consume();
		assert_eq!(b.expect_peeked_byte().unwrap(), b'Y');
		b.consume();
		assert!(b.expect_peeked_byte().is_err());
	}

	#[test]
	fn test_skip_whitespace() {
		let reader = Cursor::new(vec![b' ', b'\t', b'\n', b'A', b'B']);
		let mut b = ByteIterator::from_reader(reader, false);

		b.skip_whitespace();
		assert_eq!(b.consume(), Some(b'A'));
		assert_eq!(b.consume(), Some(b'B'));
	}

	#[test]
	fn test_into_string() {
		let reader = Cursor::new(vec![b'H', b'e', b'l', b'l', b'o']);
		let b = ByteIterator::from_reader(reader, false);

		assert_eq!(b.into_string().unwrap(), "Hello");
	}

	#[test]
	fn test_debug_error_formatting() {
		let reader = Cursor::new(vec![b'R', b'u', b's', b't']);
		let mut b = ByteIterator::from_reader(reader, true);

		b.consume(); // R
		b.consume(); // u
		b.consume(); // s
		let error = b.format_error("Testing error");

		assert!(format!("{error}").contains("Testing error at position"));
	}

	#[test]
	fn test_debug_ring_buffer() {
		let reader = Cursor::new(vec![b'a'; DEBUG_RING_BUFFER_SIZE + 5]);
		let mut b = ByteIterator::from_reader(reader, true);

		for _ in 0..DEBUG_RING_BUFFER_SIZE + 5 {
			b.consume();
		}

		assert_eq!(b.debug_buffer.len(), DEBUG_RING_BUFFER_SIZE);
	}
}

//! A byte-level iterator over a reader source with optional debug support.
//!
//! The `ByteIterator` struct provides an iterator interface over a byte stream from any type implementing `std::io::Read`.
//! It supports peeking at the next byte without consuming it, advancing the iterator, and consuming bytes one by one.
//! When debug mode is enabled, it maintains a ring buffer of recently read bytes to help with error reporting.

use anyhow::{Error, Result, anyhow};
use std::io::Read;

const DEBUG_RING_BUFFER_SIZE: usize = 16;
const BUFFER_SIZE: usize = 4096;

/// An iterator over bytes from a reader source with support for peeking, consuming, and error reporting.
///
/// # Fields
///
/// * `buffer` - Internal buffer for reading bytes from the source.
/// * `buffer_len` - Number of valid bytes currently in the buffer.
/// * `buffer_pos` - Current position within the buffer.
/// * `source` - The underlying byte source implementing `Read`.
/// * `peeked_byte` - The next byte to be consumed, if any.
/// * `position` - The current absolute position in the byte stream.
/// * `is_debug_enabled` - Flag indicating if debug mode is active.
/// * `debug_buffer` - Ring buffer storing recently read bytes for debugging purposes.
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
	/// Creates a new `ByteIterator` from a reader source.
	///
	/// # Arguments
	///
	/// * `reader` - The source implementing `Read` to iterate bytes from.
	/// * `debug` - Enables debug mode which maintains a ring buffer of recently read bytes.
	///
	/// # Returns
	///
	/// A new instance of `ByteIterator`.
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

	/// Formats an error message including the current byte position and optionally a debug snapshot of recent bytes.
	///
	/// # Arguments
	///
	/// * `msg` - The error message to include.
	///
	/// # Returns
	///
	/// An `anyhow::Error` containing the formatted error message.
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

	/// Returns the current absolute position in the byte stream.
	///
	/// # Returns
	///
	/// The current byte position as a `usize`.
	#[inline]
	#[must_use]
	pub fn position(&self) -> usize {
		self.position
	}

	/// Peeks at the next byte without consuming it.
	///
	/// # Returns
	///
	/// An `Option<u8>` containing the next byte if available, or `None` if at the end of the stream.
	#[inline]
	#[must_use]
	pub fn peek(&self) -> Option<u8> {
		self.peeked_byte
	}

	/// Advances the iterator to the next byte, updating the peeked byte and debug buffer if enabled.
	///
	/// This method consumes the current peeked byte and loads the next one.
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

	/// Consumes and returns the current peeked byte, advancing the iterator.
	///
	/// # Returns
	///
	/// An `Option<u8>` containing the consumed byte if available, or `None` if at the end of the stream.
	#[inline]
	pub fn consume(&mut self) -> Option<u8> {
		let current_byte = self.peeked_byte;
		self.advance();
		current_byte
	}

	/// Expects and returns the next byte, advancing the iterator.
	///
	/// # Errors
	///
	/// Returns an error if the end of the stream is reached unexpectedly.
	///
	/// # Returns
	///
	/// A `Result<u8>` containing the next byte or an error.
	#[inline]
	pub fn expect_next_byte(&mut self) -> Result<u8> {
		if let Some(current_byte) = self.peeked_byte {
			self.advance();
			Ok(current_byte)
		} else {
			Err(self.format_error("unexpected end"))
		}
	}

	/// Returns the current peeked byte without advancing.
	///
	/// # Errors
	///
	/// Returns an error if the end of the stream is reached unexpectedly.
	///
	/// # Returns
	///
	/// A `Result<u8>` containing the current peeked byte or an error.
	#[inline]
	pub fn expect_peeked_byte(&self) -> Result<u8> {
		self.peeked_byte.ok_or_else(|| self.format_error("unexpected end"))
	}

	/// Skips over any ASCII whitespace bytes, advancing the iterator until a non-whitespace byte or end is reached.
	pub fn skip_whitespace(&mut self) {
		while let Some(byte) = self.peek() {
			if !byte.is_ascii_whitespace() {
				break;
			}
			self.advance();
		}
	}

	/// Consumes all remaining bytes and collects them into a UTF-8 `String`.
	///
	/// # Errors
	///
	/// Returns an error if the collected bytes are not valid UTF-8.
	///
	/// # Returns
	///
	/// A `Result<String>` containing the collected string or an error.
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

//! A ceiling on how much data one decompression may produce.
//!
//! Compressed data is untrusted input: a container's metadata, its indexes and
//! every tile in it arrive compressed, and the ratios involved are not bounded
//! by anything the format guarantees. Brotli and gzip both turn a few kilobytes
//! of zeroes into gigabytes, so decompressing into an unbounded `Vec` lets a
//! small crafted file decide how much memory this process asks the operating
//! system for — and a failed allocation aborts rather than returning an error.
//!
//! The ceiling is deliberately far above anything the formats produce in
//! practice. The largest blob VersaTiles decompresses in one call is a
//! `.versatiles` block index — twelve bytes per tile, at most 65,536 tiles per
//! block, so under a megabyte — with tiles themselves smaller again. 256 MiB
//! leaves three orders of magnitude of headroom while still bounding the damage
//! a bomb can do.

use std::io::{self, Write};

use super::super::io::retry::env_u64;

/// Ceiling applied when `VERSATILES_MAX_DECOMPRESSED_BYTES` is unset: 256 MiB.
const DEFAULT_MAX_DECOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;

/// The configured ceiling; `0` disables the check.
#[must_use]
pub fn max_decompressed_bytes() -> u64 {
	env_u64("VERSATILES_MAX_DECOMPRESSED_BYTES", DEFAULT_MAX_DECOMPRESSED_BYTES)
}

/// An in-memory sink that refuses to grow past `limit` bytes.
///
/// The error surfaces as an `io::Error`, which is what the decoders propagate,
/// and reaches the caller as a normal `Result` — the point of the exercise,
/// since the alternative is an allocation failure the caller cannot catch.
pub struct LimitedWriter {
	buffer: Vec<u8>,
	limit: u64,
}

impl LimitedWriter {
	/// A sink bounded by `limit` bytes, where `0` means unbounded.
	#[must_use]
	pub fn new(limit: u64) -> Self {
		Self {
			buffer: Vec::new(),
			limit,
		}
	}

	/// The bytes written so far.
	#[must_use]
	pub fn into_vec(self) -> Vec<u8> {
		self.buffer
	}
}

impl Write for LimitedWriter {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		if self.limit > 0 {
			let written = self.buffer.len() as u64 + buf.len() as u64;
			if written > self.limit {
				return Err(io::Error::new(
					io::ErrorKind::InvalidData,
					format!(
						"decompressed data exceeds the limit of {} bytes. \
						 If this input really is that large, raise VERSATILES_MAX_DECOMPRESSED_BYTES \
						 (or set it to 0 to remove the limit)",
						self.limit
					),
				));
			}
		}
		self.buffer.extend_from_slice(buf);
		Ok(buf.len())
	}

	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn writes_up_to_the_limit() {
		let mut writer = LimitedWriter::new(8);
		writer.write_all(b"1234").unwrap();
		writer.write_all(b"5678").unwrap();
		assert_eq!(writer.into_vec(), b"12345678");
	}

	#[test]
	fn refuses_to_grow_past_the_limit() {
		let mut writer = LimitedWriter::new(8);
		writer.write_all(b"12345").unwrap();
		let error = writer.write_all(b"6789").unwrap_err();
		assert_eq!(error.kind(), io::ErrorKind::InvalidData);
		assert!(error.to_string().contains("exceeds the limit of 8 bytes"), "{error}");
		// Nothing from the rejected write is kept.
		assert_eq!(writer.into_vec(), b"12345");
	}

	#[test]
	fn zero_means_unbounded() {
		let mut writer = LimitedWriter::new(0);
		writer.write_all(&vec![0u8; 10_000]).unwrap();
		assert_eq!(writer.into_vec().len(), 10_000);
	}

	#[test]
	fn default_limit_is_used_when_unset() {
		// The suite never sets the variable; this pins the documented default.
		assert_eq!(max_decompressed_bytes(), 256 * 1024 * 1024);
	}
}

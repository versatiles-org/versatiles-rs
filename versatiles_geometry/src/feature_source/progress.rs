//! Byte-level progress tracking for [`FeatureSource`](super::FeatureSource)
//! adapters.
//!
//! Adapters open files of arbitrary size; for big inputs we want to drive a
//! progress indicator. Plumbing a concrete `ProgressHandle` through here would
//! pull `versatiles_container` into `versatiles_geometry`, so we accept a plain
//! callback instead. Each adapter exposes a builder-style `with_progress`
//! method that stores the callback and wraps its file in [`ProgressReader`]
//! during [`FeatureSource::load`](super::FeatureSource::load).

use std::{
	io::{Read, Result as IoResult, Seek, SeekFrom},
	sync::Arc,
};

/// Callback invoked with the number of bytes consumed from the underlying
/// reader on each successful read. Cloneable so the same callback can be
/// shared across multiple wrapped readers (e.g. shapefile's three sidecars).
pub type ProgressCallback = Arc<dyn Fn(u64) + Send + Sync>;

/// Wraps a [`Read`] (and forwards [`Seek`]) and reports the number of bytes
/// read to a [`ProgressCallback`] after every successful read.
pub struct ProgressReader<R> {
	inner: R,
	callback: Option<ProgressCallback>,
}

impl<R> ProgressReader<R> {
	/// Construct a wrapper that always reports to `callback`.
	#[must_use]
	pub fn new(inner: R, callback: ProgressCallback) -> Self {
		Self {
			inner,
			callback: Some(callback),
		}
	}

	/// Construct a wrapper that reports only when `callback` is `Some`.
	/// Convenient when the caller may or may not have built a progress bar.
	#[must_use]
	pub fn maybe(inner: R, callback: Option<ProgressCallback>) -> Self {
		Self { inner, callback }
	}
}

impl<R: Read> Read for ProgressReader<R> {
	fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
		let n = self.inner.read(buf)?;
		if let Some(cb) = &self.callback {
			cb(n as u64);
		}
		Ok(n)
	}
}

impl<R: Seek> Seek for ProgressReader<R> {
	fn seek(&mut self, pos: SeekFrom) -> IoResult<u64> {
		// Seeking does not consume bytes; pass through unchanged.
		self.inner.seek(pos)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::{
		io::Cursor,
		sync::atomic::{AtomicU64, Ordering},
	};

	#[test]
	fn callback_sees_bytes_read() {
		let counter = Arc::new(AtomicU64::new(0));
		let cb_counter = Arc::clone(&counter);
		let cb: ProgressCallback = Arc::new(move |n| {
			cb_counter.fetch_add(n, Ordering::Relaxed);
		});
		let mut reader = ProgressReader::new(Cursor::new(b"hello world".to_vec()), cb);
		let mut buf = [0u8; 5];
		reader.read_exact(&mut buf).unwrap();
		reader.read_exact(&mut buf).unwrap();
		assert_eq!(counter.load(Ordering::Relaxed), 10);
	}

	#[test]
	fn maybe_without_callback_is_a_passthrough() {
		let mut reader: ProgressReader<Cursor<Vec<u8>>> = ProgressReader::maybe(Cursor::new(b"abc".to_vec()), None);
		let mut buf = [0u8; 3];
		reader.read_exact(&mut buf).unwrap();
		assert_eq!(&buf, b"abc");
	}
}

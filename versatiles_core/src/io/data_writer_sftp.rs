use super::sftp_utils::{self, SftpKeepalive, SharedSession};
use super::{DataWriterTrait, network_writer::NetworkWriter};
use crate::{Blob, ByteRange};
use anyhow::{Context, Result};
use reqwest::Url;
use ssh2::{OpenFlags, OpenType};
use std::{
	io::{Seek, SeekFrom, Write},
	path::{Path, PathBuf},
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

/// Size of the in-memory append buffer. The `.versatiles`/`pmtiles` writers
/// append once per tile (often only a few KB); over SFTP each unbuffered write
/// is a separate, round-trip-bound libssh2 request, which collapses throughput.
/// Coalescing appends into large writes of this size amortizes that overhead.
const BUFFER_CAPACITY: usize = 16 * 1024 * 1024;

/// Render a byte count as a human-readable string for diagnostics.
fn format_bytes(n: u64) -> String {
	const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
	let mut value = n as f64;
	let mut unit = 0;
	while value >= 1024.0 && unit < UNITS.len() - 1 {
		value /= 1024.0;
		unit += 1;
	}
	format!("{value:.1} {}", UNITS[unit])
}

/// A struct that provides writing capabilities to a remote file via SFTP.
pub struct DataWriterSftp {
	file: ssh2::File,
	/// Position of the last byte flushed to the remote file (excludes `buffer`).
	position: u64,
	/// Pending appended bytes not yet flushed to the network. Coalesced into a
	/// single large `write` once it reaches `BUFFER_CAPACITY` or on flush.
	buffer: Vec<u8>,
	url: Url,
	identity_file: Option<PathBuf>,
	name: String,
	// Shared SSH session (swapped on reconnect). Shared with the background keepalive,
	// which pings it during idle gaps so the peer does not reap the connection.
	session: SharedSession,
	// Background keepalive; dropping it stops and joins the pinger thread.
	_keepalive: SftpKeepalive,
	// --- Connection diagnostics (reset on every (re)connect) ---
	/// When the current connection was established.
	connected_at: Instant,
	/// Bytes appended over the current connection.
	bytes_on_connection: u64,
	/// When the last successful write finished — used to measure the idle gap
	/// before the next write (e.g. while a block is being read/processed upstream).
	last_write_end: Instant,
	/// Idle gap immediately before the most recent write attempt.
	last_attempt_idle: Duration,
}

impl DataWriterSftp {
	/// Opens a remote file for writing via SFTP.
	///
	/// # Arguments
	/// * `url` - A parsed SFTP URL
	///
	/// # Authentication priority
	/// 1. Credentials in URL (password auth)
	/// 2. SSH agent
	/// 3. Default key files (~/.ssh/id_ed25519, id_rsa, id_ecdsa)
	pub fn from_url(url: &Url, identity_file: Option<&Path>) -> Result<Self> {
		let session = sftp_utils::open_session(url, identity_file)?;
		let path = sftp_utils::remote_path(url);

		let sftp = session.sftp()?;
		let file = sftp
			.create(&path)
			.with_context(|| format!("failed to create remote file {path:?}"))?;

		let name = sftp_utils::display_name(url);
		let session: SharedSession = Arc::new(Mutex::new(session));
		let keepalive = SftpKeepalive::start(Arc::clone(&session), name.clone());

		let now = Instant::now();
		Ok(DataWriterSftp {
			file,
			position: 0,
			buffer: Vec::with_capacity(BUFFER_CAPACITY),
			url: url.clone(),
			identity_file: identity_file.map(Path::to_path_buf),
			name,
			session,
			_keepalive: keepalive,
			connected_at: now,
			bytes_on_connection: 0,
			last_write_end: now,
			last_attempt_idle: Duration::ZERO,
		})
	}

	/// Returns the remote path extracted from an SFTP URL (for extension detection).
	#[must_use]
	pub fn path_from_url(url: &Url) -> PathBuf {
		sftp_utils::remote_path(url)
	}

	/// Flushes the pending append buffer to the remote file as a single (retried)
	/// network write. No-op when the buffer is empty. Keeps the buffer's capacity
	/// for reuse.
	fn flush_buffer(&mut self) -> Result<()> {
		if self.buffer.is_empty() {
			return Ok(());
		}
		let blob = Blob::from(self.buffer.as_slice());
		self.network_append(&blob)?;
		self.buffer.clear();
		Ok(())
	}
}

impl NetworkWriter for DataWriterSftp {
	fn try_append(&mut self, blob: &Blob) -> Result<ByteRange> {
		// Idle gap since the last successful write (time spent producing this block
		// upstream, during which the SFTP session sat idle).
		self.last_attempt_idle = self.last_write_end.elapsed();
		let pos = self.position;
		self.file.write_all(blob.as_slice())?;
		self.position += blob.len();
		self.bytes_on_connection += blob.len();
		self.last_write_end = Instant::now();
		Ok(ByteRange::new(pos, blob.len()))
	}

	fn try_write_at(&mut self, offset: u64, blob: &Blob, restore_pos: u64) -> Result<()> {
		self
			.file
			.seek(SeekFrom::Start(offset))
			.with_context(|| format!("failed to seek to offset {offset} in '{}'", self.name))?;
		self.file.write_all(blob.as_slice()).with_context(|| {
			format!(
				"failed to write {} bytes at offset {offset} in '{}'",
				blob.len(),
				self.name
			)
		})?;
		self
			.file
			.seek(SeekFrom::Start(restore_pos))
			.with_context(|| format!("failed to seek back to position {restore_pos} in '{}'", self.name))?;
		Ok(())
	}

	fn try_seek(&mut self, position: u64) -> Result<()> {
		self
			.file
			.seek(SeekFrom::Start(position))
			.with_context(|| format!("failed to seek to position {position} in '{}'", self.name))?;
		self.position = position;
		Ok(())
	}

	fn reconnect(&mut self) -> Result<()> {
		let path = sftp_utils::remote_path(&self.url);
		// Summarize the connection that just died — this is the key signal for
		// diagnosing *why* the server drops us: compare "alive" (time-based limit?),
		// "wrote" (byte-volume limit?) and "idle before failure" (idle-timeout?)
		// across reconnects.
		log::info!(
			"reconnecting SFTP writer to '{}' (previous connection: alive {:.1}s, wrote {}, idle {:.1}s before failure)",
			self.name,
			self.connected_at.elapsed().as_secs_f64(),
			format_bytes(self.bytes_on_connection),
			self.last_attempt_idle.as_secs_f64(),
		);

		let session = sftp_utils::open_session(&self.url, self.identity_file.as_deref())?;
		let sftp = session.sftp()?;
		let mut file = sftp
			.open_mode(&path, OpenFlags::WRITE, 0o644, OpenType::File)
			.with_context(|| format!("failed to reopen remote file {path:?} for writing"))?;
		file
			.seek(SeekFrom::Start(self.position))
			.with_context(|| format!("failed to seek to position {} in {path:?}", self.position))?;

		self.file = file;
		// Swap the shared session so the keepalive thread pings the new connection.
		*self.session.lock().expect("session mutex poisoned") = session;
		let now = Instant::now();
		self.connected_at = now;
		self.bytes_on_connection = 0;
		self.last_write_end = now;
		Ok(())
	}

	fn writer_name(&self) -> &str {
		&self.name
	}

	fn tracked_position(&self) -> u64 {
		self.position
	}

	fn failure_context(&self) -> String {
		format!(
			" [conn alive {:.1}s, {} written, idle {:.1}s before this write]",
			self.connected_at.elapsed().as_secs_f64(),
			format_bytes(self.bytes_on_connection),
			self.last_attempt_idle.as_secs_f64(),
		)
	}
}

impl DataWriterTrait for DataWriterSftp {
	/// Appends to the in-memory buffer, flushing to the network once it reaches
	/// `BUFFER_CAPACITY`. The returned range uses the *logical* offset (where the
	/// bytes will land in the file once flushed); appends are sequential and the
	/// buffer flushes in order, so recorded offsets are correct.
	fn append(&mut self, blob: &Blob) -> Result<ByteRange> {
		let offset = self.position + self.buffer.len() as u64;
		self.buffer.extend_from_slice(blob.as_slice());
		if self.buffer.len() >= BUFFER_CAPACITY {
			self.flush_buffer()?;
		}
		Ok(ByteRange::new(offset, blob.len()))
	}

	fn write_start(&mut self, blob: &Blob) -> Result<()> {
		// Flush so the file holds all appended bytes before patching the start.
		self.flush_buffer()?;
		self.network_write_start(blob)
	}

	fn position(&mut self) -> Result<u64> {
		// Logical position includes bytes still pending in the buffer.
		Ok(self.position + self.buffer.len() as u64)
	}

	fn set_position(&mut self, position: u64) -> Result<()> {
		// Flush before seeking so buffered appends are not misplaced.
		self.flush_buffer()?;
		self.network_set_position(position)
	}

	fn finalize(&mut self) -> Result<()> {
		self.flush_buffer()
	}
}

impl Drop for DataWriterSftp {
	fn drop(&mut self) {
		// `finalize()` is the supported way to flush; warn if it was missed so an
		// incomplete upload is at least diagnosable. We do not attempt network I/O
		// here (it could block/fail during unwinding).
		if !self.buffer.is_empty() {
			log::warn!(
				"SFTP writer for '{}' dropped with {} unflushed; call finalize() before dropping",
				self.name,
				format_bytes(self.buffer.len() as u64),
			);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_path_from_url() {
		let url = Url::parse("sftp://host/data/out.versatiles").unwrap();
		assert_eq!(
			DataWriterSftp::path_from_url(&url),
			PathBuf::from("/data/out.versatiles")
		);
	}

	#[test]
	fn test_path_from_url_with_credentials() {
		let url = Url::parse("sftp://user:pass@host:2222/output/tiles.tar").unwrap();
		assert_eq!(DataWriterSftp::path_from_url(&url), PathBuf::from("/output/tiles.tar"));
	}

	#[test]
	fn test_path_from_url_root() {
		let url = Url::parse("sftp://host/file.versatiles").unwrap();
		assert_eq!(DataWriterSftp::path_from_url(&url), PathBuf::from("/file.versatiles"));
	}

	#[test]
	fn test_path_from_url_nested() {
		let url = Url::parse("sftp://host/a/b/c/d.tar").unwrap();
		assert_eq!(DataWriterSftp::path_from_url(&url), PathBuf::from("/a/b/c/d.tar"));
	}

	#[test]
	fn test_from_url_unreachable_host() {
		let url = Url::parse("sftp://192.0.2.1:22222/path/file.versatiles").unwrap();
		let result = DataWriterSftp::from_url(&url, None);
		assert!(result.is_err());
	}

	#[cfg(all(feature = "ssh2", unix))]
	mod sftp_server_tests {
		use super::*;
		use crate::{Blob, io::test_sftp_server::TestSftpServer};

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn append_writes_bytes() {
			let server = TestSftpServer::start().await;
			let url = server.url("/out.bin");
			tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
				let mut w = DataWriterSftp::from_url(&url, None)?;
				w.append(&Blob::from(b"hello"))?;
				w.append(&Blob::from(b"world"))?;
				w.finalize()?;
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();
			assert_eq!(server.read_file("/out.bin").await, b"helloworld");
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn write_start_overwrites_beginning() {
			let server = TestSftpServer::start().await;
			let url = server.url("/out.bin");
			tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
				let mut w = DataWriterSftp::from_url(&url, None)?;
				w.append(&Blob::from(b"AAAAABBBBB"))?;
				w.write_start(&Blob::from(b"12345"))?;
				w.finalize()?;
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();
			assert_eq!(server.read_file("/out.bin").await, b"12345BBBBB");
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn position_tracking() {
			let server = TestSftpServer::start().await;
			let url = server.url("/out.bin");
			tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
				let mut w = DataWriterSftp::from_url(&url, None)?;
				assert_eq!(w.position()?, 0);
				w.append(&Blob::from(b"abc"))?;
				assert_eq!(w.position()?, 3);
				w.append(&Blob::from(b"de"))?;
				assert_eq!(w.position()?, 5);
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn set_position_then_append() {
			let server = TestSftpServer::start().await;
			let url = server.url("/out.bin");
			tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
				let mut w = DataWriterSftp::from_url(&url, None)?;
				w.append(&Blob::from(vec![0u8; 10]))?;
				w.set_position(5)?;
				w.append(&Blob::from(vec![1u8; 5]))?;
				w.finalize()?;
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();
			assert_eq!(server.read_file("/out.bin").await, [0, 0, 0, 0, 0, 1, 1, 1, 1, 1]);
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn write_retry_after_disconnect() {
			// Longer timeout: the initial handshake + reconnect after the injected
			// disconnect can exceed 500 ms on a loaded CI runner.
			let server = TestSftpServer::start().await;
			let url = server.url("/out.bin");
			let mut writer = tokio::task::spawn_blocking(move || DataWriterSftp::from_url(&url, None))
				.await
				.unwrap()
				.unwrap();
			server.schedule_disconnect();
			tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
				writer.append(&Blob::from(b"hello"))?;
				// The buffered append is written (and the disconnect retried) on flush.
				writer.finalize()?;
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();
			assert_eq!(server.read_file("/out.bin").await, b"hello");
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn many_small_appends_are_coalesced_and_flushed_on_finalize() {
			let server = TestSftpServer::start().await;
			let url = server.url("/out.bin");
			tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
				let mut w = DataWriterSftp::from_url(&url, None)?;
				// Many tiny appends (well under BUFFER_CAPACITY) stay buffered until finalize.
				for i in 0..1000u32 {
					let range = w.append(&Blob::from(i.to_le_bytes().to_vec()))?;
					// Offsets are logical (sequential) even though nothing was flushed yet.
					assert_eq!(range.offset, u64::from(i) * 4);
				}
				assert_eq!(w.position()?, 4000);
				w.finalize()?;
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();

			let bytes = server.read_file("/out.bin").await;
			assert_eq!(bytes.len(), 4000);
			let mut expected = Vec::with_capacity(4000);
			for i in 0..1000u32 {
				expected.extend_from_slice(&i.to_le_bytes());
			}
			assert_eq!(bytes, expected);
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn append_larger_than_buffer_capacity_flushes() {
			let server = TestSftpServer::start().await;
			let url = server.url("/out.bin");
			let big = vec![7u8; BUFFER_CAPACITY + 1024];
			let expected = big.clone();
			tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
				let mut w = DataWriterSftp::from_url(&url, None)?;
				// A single append exceeding the buffer triggers an immediate flush.
				w.append(&Blob::from(big))?;
				w.finalize()?;
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();
			assert_eq!(server.read_file("/out.bin").await, expected);
		}

		/// End-to-end round trip mirroring how the `.versatiles` container writer
		/// uses this writer: reserve a header region, append many small "tiles"
		/// (buffered) recording each reported `ByteRange`, patch the real header in
		/// at offset 0 via `write_start` (which flushes the buffer), then finalize.
		/// Reading every reported range back through the **real** SFTP reader proves
		/// the buffered writer's logical offsets are exactly where the reader finds
		/// the bytes — the contract the container's block index depends on.
		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn buffered_writer_offsets_resolve_with_real_reader() {
			use crate::io::{DataReaderSftp, DataReaderTrait};

			let server = TestSftpServer::start().await;
			let url = server.url("/round_trip.bin");
			let read_url = url.clone();

			let (header_range, chunks) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
				let mut w = DataWriterSftp::from_url(&url, None)?;

				// Reserve a 16-byte header region at the start (offset 0).
				let header_range = w.append(&Blob::from(vec![0u8; 16]))?;
				assert_eq!(header_range.offset, 0);

				// Many small "tiles", each recording the ByteRange the writer reports.
				let mut chunks = Vec::new();
				for i in 0..300u32 {
					let payload = format!("tile-{i:05}-payload").into_bytes();
					let range = w.append(&Blob::from(payload.clone()))?;
					chunks.push((range, payload));
				}

				// Patch the real header in at offset 0 (flushes the buffer first), then finalize.
				w.write_start(&Blob::from(b"VERSATILES\0\0\0\0\0\0".to_vec()))?;
				w.finalize()?;
				Ok((header_range, chunks))
			})
			.await
			.unwrap()
			.unwrap();

			let reader = tokio::task::spawn_blocking(move || DataReaderSftp::open(&read_url, None))
				.await
				.unwrap()
				.unwrap();

			// The header lands at offset 0 (overwriting the reserved region).
			let header = reader.read_range(&header_range).await.unwrap();
			assert_eq!(header.as_slice(), b"VERSATILES\0\0\0\0\0\0");

			// Every tile reads back at the offset the writer reported for it.
			for (range, payload) in chunks {
				let got = reader.read_range(&range).await.unwrap();
				assert_eq!(
					got.as_slice(),
					payload.as_slice(),
					"mismatch at offset {}",
					range.offset
				);
			}
		}
	}
}

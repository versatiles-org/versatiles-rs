//! This module provides functionality for reading data from HTTP endpoints.
//!
//! # Overview
//!
//! The `DataReaderHttp` struct allows for reading data from HTTP and HTTPS URLs. It implements the
//! `DataReaderTrait` to provide asynchronous reading capabilities. The module ensures the URL has
//! a valid scheme (`http` or `https`) and uses the `reqwest` library to handle HTTP requests.
//!
//! # Examples
//!
//! ```rust,no_run
//! use versatiles_core::{io::{DataReaderHttp, DataReaderTrait}, Blob, ByteRange};
//! use anyhow::Result;
//! use reqwest::Url;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let url = Url::parse("https://example.com/data.bin").unwrap();
//!     let mut reader = DataReaderHttp::try_from(&url)?;
//!
//!     // Reading a range of data
//!     let range = ByteRange::new(0, 15);
//!     let partial_data = reader.read_range(&range).await?;
//!
//!     // Process the data
//!     println!("Read {} bytes", partial_data.len());
//!
//!     Ok(())
//! }
//! ```

use std::{
	fmt, str,
	sync::{Arc, LazyLock, atomic::AtomicU64},
	time::Duration,
};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use dashmap::DashMap;
use percent_encoding::percent_decode_str;
use regex::{Regex, RegexBuilder};
use reqwest::{Client, RequestBuilder, StatusCode, Url};
use tokio::{sync::Semaphore, time::sleep};

use super::{
	DataReaderTrait,
	network_reader::{NetworkReader, SmallerRangeWontHelp},
	retry::env_u64,
};
use crate::{Blob, ByteRange};

/// Ceiling on a whole-body read when `VERSATILES_MAX_HTTP_BODY_BYTES` is unset.
///
/// Only applies to `read_all`, where nothing in the request says how much is
/// expected — a range read is bounded by the range it asked for. 256 MiB is far
/// above the style sheets, sprites and TileJSON documents fetched this way, and
/// matches the decompression ceiling next door.
const DEFAULT_MAX_BODY_BYTES: u64 = 256 * 1024 * 1024;

/// Why reading a response body stopped.
///
/// Separated from a plain error so the retry loop can tell the two apart: a
/// transport hiccup may be worth another attempt, a body that exceeds what was
/// asked for never is — the next attempt would fetch the same oversized body.
enum BodyError {
	Transport(reqwest::Error),
	TooLarge { limit: u64 },
}

/// Read a response body, refusing to buffer more than `limit` bytes.
///
/// `Response::bytes()` buffers whatever the peer sends: a server answering a
/// 64 KiB range request with ten gigabytes is answered by allocating ten
/// gigabytes, and `Content-Length` is the peer's claim rather than a
/// constraint. Reading the stream chunk by chunk with a running total stops at
/// the limit instead of after it.
async fn read_body_limited(response: reqwest::Response, limit: u64) -> std::result::Result<Vec<u8>, BodyError> {
	use futures::StreamExt;

	// Only as a starting size, and only when the peer's claim is plausible —
	// it is not trusted as a bound.
	let mut body = match response.content_length() {
		Some(len) if len <= limit => Vec::with_capacity(usize::try_from(len).unwrap_or(0)),
		_ => Vec::new(),
	};

	let mut stream = response.bytes_stream();
	while let Some(chunk) = stream.next().await {
		let chunk = chunk.map_err(BodyError::Transport)?;
		if body.len() as u64 + chunk.len() as u64 > limit {
			return Err(BodyError::TooLarge { limit });
		}
		body.extend_from_slice(&chunk);
	}

	Ok(body)
}

/// Check that a `206` response describes the range that was asked for.
///
/// Only the header. How many bytes actually follow it is a separate question,
/// checked against `range.length` once the body has been read — the two used to
/// be conflated, and a short body passed for a complete one.
fn verify_content_range(response: &reqwest::Response, range: &ByteRange) -> Result<()> {
	let content_range = response
		.headers()
		.get("content-range")
		.ok_or_else(|| anyhow!("response is missing Content-Range header"))?
		.to_str()?;

	static RE_RANGE: LazyLock<Regex> = LazyLock::new(|| {
		RegexBuilder::new(r"^bytes (\d+)-(\d+)/\d+$")
			.case_insensitive(true)
			.build()
			.expect("valid regex literal")
	});

	let caps = RE_RANGE.captures(content_range).ok_or_else(|| {
		anyhow!("unexpected Content-Range format: '{content_range}', expected 'bytes <start>-<end>/<total>'")
	})?;
	let content_range_start: u64 = caps[1].parse()?;
	let content_range_end: u64 = caps[2].parse()?;

	if content_range_start != range.offset {
		bail!(
			"Content-Range start mismatch: expected {}, got {content_range_start}",
			range.offset
		);
	}

	let expected_end = range.end()? - 1;
	if content_range_end != expected_end {
		bail!("Content-Range end mismatch: expected {expected_end}, got {content_range_end}");
	}

	Ok(())
}

/// Maximum number of HTTP requests allowed in flight, shared across all
/// readers pointing at the same host.
///
/// Caps the upstream burst when many `read_range` calls fan out concurrently
/// (e.g. via `buffer_unordered` in tile-chunk streaming) AND when multiple
/// `DataReaderHttp` instances point at the same origin (e.g. stacking many
/// PMTiles from one server). Prevents 429/503 responses from origins with
/// per-IP rate limits and keeps the adaptive `max_request_bytes` splitter
/// from misinterpreting overload as oversize.
///
/// 8 is a conservative default chosen to work well with self-hosted origins
/// and modest CDNs. Cloud object stores (S3, R2, GCS) can sustain more — bump
/// per reader with [`DataReaderHttp::with_max_in_flight`] when known.
const DEFAULT_MAX_IN_FLIGHT: usize = 8;

/// Per-host semaphore registry. Two `DataReaderHttp` instances built from
/// URLs with the same host share the same `Semaphore` instance, so opening
/// many readers against one origin doesn't multiply the in-flight cap.
///
/// The semaphore for a host is created on first access with
/// `DEFAULT_MAX_IN_FLIGHT` permits. Subsequent readers for the same host
/// reuse it regardless of their individual configured cap — callers that
/// need to opt out can use [`DataReaderHttp::with_max_in_flight`], which
/// installs a private per-reader semaphore.
static HOST_SEMAPHORES: LazyLock<DashMap<String, Arc<Semaphore>>> = LazyLock::new(DashMap::new);

fn host_semaphore(url: &Url) -> Arc<Semaphore> {
	// Fall back to the full URL string if the URL somehow has no host. This
	// gives unique-per-URL isolation rather than crashing or sharing globally.
	let key = url.host_str().unwrap_or_else(|| url.as_str()).to_string();
	HOST_SEMAPHORES
		.entry(key)
		.or_insert_with(|| Arc::new(Semaphore::new(DEFAULT_MAX_IN_FLIGHT)))
		.clone()
}

/// A struct that provides reading capabilities from an HTTP(S) endpoint.
pub struct DataReaderHttp {
	client: Client,
	name: String,
	url: Url,
	username: Option<String>,
	password: Option<String>,
	max_request_bytes: AtomicU64,
	in_flight: Arc<Semaphore>,
}

impl fmt::Debug for DataReaderHttp {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("DataReaderHttp")
			.field("url", &self.url.as_str())
			.field("has_credentials", &self.username.is_some())
			.finish()
	}
}

impl TryFrom<&Url> for DataReaderHttp {
	type Error = anyhow::Error;

	fn try_from(url: &Url) -> Result<DataReaderHttp> {
		let mut url = url.clone();
		let username = if url.username().is_empty() {
			None
		} else {
			Some(percent_decode_str(url.username()).decode_utf8()?.into_owned())
		};

		let password: Option<String> = if let Some(p) = url.password() {
			Some(if let Ok(v) = percent_decode_str(p).decode_utf8() {
				v.into_owned()
			} else {
				bail!("failed to decode password");
			})
		} else {
			None
		};

		// Strip credentials from the URL before any logging or error messages
		url.set_username("").map_err(|_| anyhow!("failed to set username"))?;
		url.set_password(None).map_err(|_| anyhow!("failed to set password"))?;

		match url.scheme() {
			"http" | "https" => (),
			other => bail!("unsupported URL scheme '{other}' in '{url}', expected 'http' or 'https'"),
		}

		let client = Client::builder()
			.user_agent(crate::io::user_agent())
			.connect_timeout(Duration::from_secs(30))
			// No overall timeout — large range reads (100+ MB) can take minutes.
			// Dead connections are caught by TCP keepalive and connect_timeout.
			.tcp_keepalive(Duration::from_secs(60))
			.use_rustls_tls()
			.build()?;

		let in_flight = host_semaphore(&url);
		Ok(DataReaderHttp {
			client,
			name: url.to_string(),
			url,
			username,
			password,
			max_request_bytes: AtomicU64::new(u64::MAX),
			in_flight,
		})
	}
}

impl DataReaderHttp {
	fn apply_auth(&self, builder: RequestBuilder) -> RequestBuilder {
		if let Some(username) = &self.username {
			builder.basic_auth(username, self.password.as_deref())
		} else {
			builder
		}
	}

	/// Override the maximum number of HTTP requests allowed in flight.
	///
	/// Values below 2 are clamped to 2 to keep the adaptive splitter (which
	/// fans out to two concurrent halves on oversize-range failure) from
	/// deadlocking.
	#[must_use]
	pub fn with_max_in_flight(mut self, n: usize) -> Self {
		self.in_flight = Arc::new(Semaphore::new(n.max(2)));
		self
	}
}

fn is_retryable_error(err: &reqwest::Error) -> bool {
	err.is_connect() || err.is_timeout() || err.is_body()
}

/// Describe a response whose status is not `206`, marking it when splitting the
/// range would meet the same answer.
///
/// Server errors stay splittable: a server can genuinely fail on a large
/// response and manage a small one, and so can a proxy answering `413`.
/// Everything else here — a `200` from a server that ignores `Range`, a `404`,
/// a `416` for a range past the end of the file — is an answer about the
/// request rather than its size, and halving it just asks twice.
fn unexpected_status(range: &ByteRange, len: u64, url: &Url, status: StatusCode, total_attempts: u32) -> anyhow::Error {
	if status.is_server_error() {
		return anyhow!(
			"could not read {range} ({len} bytes) from '{url}': server returned {status} — gave up after {total_attempts} attempts"
		);
	}

	let error = anyhow!("could not read {range} ({len} bytes) from '{url}': expected HTTP 206, got {status}");
	if status == StatusCode::PAYLOAD_TOO_LARGE {
		error
	} else {
		error.context(SmallerRangeWontHelp)
	}
}

/// Render a `reqwest::Error` together with its full `source()` chain.
///
/// `reqwest::Error`'s Display reports the outer wrapper (e.g. "error sending
/// request for url (...)") but hides the underlying cause — connection reset,
/// TLS handshake timeout, DNS failure, etc. Walking the chain makes the actual
/// failure visible in retry/bail logs without bumping verbosity to `debug`.
fn describe_error(err: &reqwest::Error) -> String {
	use std::error::Error;
	let mut parts: Vec<String> = vec![err.to_string()];
	let mut src: Option<&dyn Error> = err.source();
	while let Some(s) = src {
		parts.push(s.to_string());
		src = s.source();
	}
	parts.join(" -> ")
}

impl DataReaderHttp {
	/// Single-range read with retry/backoff.
	async fn try_read_range_impl(&self, range: &ByteRange) -> Result<Blob> {
		// `end()` is checked: the range comes from a container header, and an
		// unchecked sum here panics in a debug build and asks for a wrapped byte
		// range in a release one.
		let request_range: String = format!("bytes={}-{}", range.offset, range.end()? - 1);
		let policy = super::retry::policy();
		let max_retries = policy.max_retries;
		let total_attempts = max_retries + 1;
		let url = &self.url;
		let len = range.length;

		for attempt in 0..=max_retries {
			let attempt_label = format!("attempt {}/{total_attempts}", attempt + 1);

			if attempt > 0 {
				let backoff = policy.backoff(attempt - 1);
				log::warn!("HTTP read {range} from '{url}': retrying ({attempt_label}, waiting {backoff:?})");
				sleep(backoff).await;
			}

			log::trace!("http: {range} {attempt_label}: sending request");

			// Acquire INSIDE the loop so retry backoff sleeps don't hold the permit,
			// and so a fatal failure releases the permit before bail!.
			let _permit = self
				.in_flight
				.clone()
				.acquire_owned()
				.await
				.expect("in-flight semaphore is never closed");

			let response = match self
				.apply_auth(self.client.get(self.url.clone()))
				.header("range", &request_range)
				.send()
				.await
			{
				Ok(r) => r,
				Err(e) if is_retryable_error(&e) && attempt < max_retries => {
					log::warn!(
						"HTTP read {range} from '{url}': {} ({attempt_label}), will retry",
						describe_error(&e)
					);
					continue;
				}
				Err(e) => {
					bail!(
						"could not read {range} ({len} bytes) from '{url}': {} — gave up after {total_attempts} attempts",
						describe_error(&e)
					)
				}
			};

			let status = response.status();
			if status.is_server_error() && attempt < max_retries {
				log::warn!("HTTP read {range} from '{url}': server returned {status} ({attempt_label}), will retry");
				continue;
			}

			if status != StatusCode::PARTIAL_CONTENT {
				return Err(unexpected_status(range, len, url, status, total_attempts));
			}

			verify_content_range(&response, range)?;

			// Bounded by the range that was asked for. `Content-Range` was checked
			// above, but that is only the header: nothing so far constrains how
			// many bytes actually follow it.
			let bytes = match read_body_limited(response, range.length).await {
				Ok(b) => b,
				Err(BodyError::Transport(e)) if is_retryable_error(&e) && attempt < max_retries => {
					log::warn!(
						"HTTP read {range} from '{url}': error reading body: {} ({attempt_label}), will retry",
						describe_error(&e)
					);
					continue;
				}
				Err(BodyError::Transport(e)) => bail!(
					"could not read {range} ({len} bytes) from '{url}': error reading body: {} — gave up after {total_attempts} attempts",
					describe_error(&e)
				),
				// Marked: a peer that sends more than it was asked for does so
				// again for each half, so splitting turns one oversized body into
				// several.
				Err(BodyError::TooLarge { limit }) => {
					return Err(
						anyhow!(
							"could not read {range} from '{url}': the response body is longer than the {limit} bytes requested"
						)
						.context(SmallerRangeWontHelp),
					);
				}
			};

			// A short body was previously concatenated into the result by
			// `split_and_read`, which turned a truncated response into a blob
			// that looked complete. The range was satisfied or it was not.
			if bytes.len() as u64 != range.length {
				// Marked for the same reason as an over-long body: the peer
				// answered promptly with the wrong thing, and asking twice for
				// halves gets two wrong answers instead of one.
				return Err(
					anyhow!(
						"could not read {range} from '{url}': expected {} bytes, the response body carried {}",
						range.length,
						bytes.len()
					)
					.context(SmallerRangeWontHelp),
				);
			}

			log::trace!("http: {range} {attempt_label}: read {} bytes ok", bytes.len());
			return Ok(Blob::from(bytes));
		}

		bail!("could not read {range} ({len} bytes) from '{url}' — gave up after {total_attempts} attempts")
	}
}

#[async_trait]
impl NetworkReader for DataReaderHttp {
	async fn try_read_range(&self, range: &ByteRange) -> Result<Blob> {
		self.try_read_range_impl(range).await
	}

	fn max_request_bytes(&self) -> &AtomicU64 {
		&self.max_request_bytes
	}
}

#[async_trait]
impl DataReaderTrait for DataReaderHttp {
	async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		self.network_read_range(range).await
	}

	/// Reads all the data from the HTTP(S) endpoint.
	///
	/// # Returns
	///
	/// * A Result containing a Blob with all the data or an error.
	async fn read_all(&self) -> Result<Blob> {
		let policy = super::retry::policy();
		let max_retries = policy.max_retries;
		let total_attempts = max_retries + 1;
		let url = &self.url;

		for attempt in 0..=max_retries {
			let attempt_label = format!("attempt {}/{total_attempts}", attempt + 1);

			if attempt > 0 {
				let backoff = policy.backoff(attempt - 1);
				log::warn!("HTTP read from '{url}': retrying ({attempt_label}, waiting {backoff:?})");
				sleep(backoff).await;
			}

			let _permit = self
				.in_flight
				.clone()
				.acquire_owned()
				.await
				.expect("in-flight semaphore is never closed");

			let response = match self.apply_auth(self.client.get(self.url.clone())).send().await {
				Ok(r) => r,
				Err(e) if is_retryable_error(&e) && attempt < max_retries => {
					log::warn!(
						"HTTP read from '{url}': {} ({attempt_label}), will retry",
						describe_error(&e)
					);
					continue;
				}
				Err(e) => bail!(
					"could not read from '{url}': {} — gave up after {total_attempts} attempts",
					describe_error(&e)
				),
			};

			let status = response.status();
			if status.is_server_error() && attempt < max_retries {
				log::warn!("HTTP read from '{url}': server returned {status} ({attempt_label}), will retry");
				continue;
			}

			if !status.is_success() {
				if status.is_server_error() {
					bail!("could not read from '{url}': server returned {status} — gave up after {total_attempts} attempts");
				}
				bail!("could not read from '{url}': server returned {status}");
			}

			let limit = env_u64("VERSATILES_MAX_HTTP_BODY_BYTES", DEFAULT_MAX_BODY_BYTES);
			let bytes = match read_body_limited(response, limit).await {
				Ok(b) => b,
				Err(BodyError::Transport(e)) if is_retryable_error(&e) && attempt < max_retries => {
					log::warn!(
						"HTTP read from '{url}': error reading body: {} ({attempt_label}), will retry",
						describe_error(&e)
					);
					continue;
				}
				Err(BodyError::Transport(e)) => {
					bail!(
						"could not read from '{url}': error reading body: {} — gave up after {total_attempts} attempts",
						describe_error(&e)
					)
				}
				Err(BodyError::TooLarge { limit }) => bail!(
					"could not read from '{url}': the response body exceeds {limit} bytes \
					 (raise VERSATILES_MAX_HTTP_BODY_BYTES if that is expected)"
				),
			};

			return Ok(Blob::from(bytes));
		}

		bail!("could not read from '{url}' — gave up after {total_attempts} attempts")
	}

	/// Gets the name of the data source.
	///
	/// # Returns
	///
	/// * A string slice representing the name of the data source.
	fn name(&self) -> &str {
		&self.name
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// Test the 'new' method for valid and invalid URLs
	#[test]
	fn new() {
		let valid_url = Url::parse("https://www.example.com").unwrap();
		let invalid_url = Url::parse("ftp://www.example.com").unwrap();

		// Test with a valid URL
		let data_reader_http = DataReaderHttp::try_from(&valid_url);
		assert!(data_reader_http.is_ok());

		// Test with an invalid URL
		let data_reader_http = DataReaderHttp::try_from(&invalid_url);
		assert!(data_reader_http.is_err());
	}

	async fn read_range_helper(url: &str, offset: u64, length: u64, expected: &str) -> Result<()> {
		let url = Url::parse(url).unwrap();
		let data_reader_http = DataReaderHttp::try_from(&url)?;

		// Define a range to read
		let range = ByteRange { offset, length };

		// Read the specified range from the URL
		let blob = data_reader_http.read_range(&range).await?;

		// Convert the resulting Blob to a string
		let result_text = str::from_utf8(blob.as_slice())?;

		// Check if the read range matches the expected text
		assert_eq!(result_text, expected);

		Ok(())
	}

	#[tokio::test]
	async fn read_range_git() {
		read_range_helper(
			"https://raw.githubusercontent.com/versatiles-org/versatiles-rs/refs/heads/main/testdata/berlin.mbtiles",
			7,
			8,
			"format 3",
		)
		.await
		.unwrap();
	}

	#[tokio::test]
	async fn read_range_google() {
		read_range_helper("https://google.com/", 100, 110, "plingplong")
			.await
			.unwrap_err();
	}

	// Test the 'get_name' method
	#[test]
	fn get_name() -> Result<()> {
		let url = "https://www.example.com/";
		let data_reader_http = DataReaderHttp::try_from(&Url::parse(url).unwrap())?;

		// Check if the name matches the original URL
		assert_eq!(data_reader_http.name(), url);

		Ok(())
	}

	#[test]
	fn from_url_with_credentials() -> Result<()> {
		let url = Url::parse("https://user:p%40ss@example.com/data.bin").unwrap();
		let reader = DataReaderHttp::try_from(&url)?;

		assert_eq!(reader.username.as_deref(), Some("user"));
		assert_eq!(reader.password.as_deref(), Some("p@ss"));
		assert_eq!(reader.name(), "https://example.com/data.bin");
		assert_eq!(reader.url.username(), "");
		assert_eq!(reader.url.password(), None);

		Ok(())
	}

	#[test]
	fn from_url_without_credentials() -> Result<()> {
		let url = Url::parse("https://example.com/data.bin").unwrap();
		let reader = DataReaderHttp::try_from(&url)?;

		assert_eq!(reader.username, None);
		assert_eq!(reader.password, None);
		assert_eq!(reader.name(), "https://example.com/data.bin");

		Ok(())
	}

	#[test]
	fn debug_impl_hides_credentials() -> Result<()> {
		let with_creds = DataReaderHttp::try_from(&Url::parse("https://user:pass@example.com/").unwrap())?;
		let debug = format!("{with_creds:?}");
		assert!(debug.contains("has_credentials: true"));
		assert!(!debug.contains("pass"));

		let no_creds = DataReaderHttp::try_from(&Url::parse("https://example.com/").unwrap())?;
		let debug = format!("{no_creds:?}");
		assert!(debug.contains("has_credentials: false"));

		Ok(())
	}

	#[test]
	fn from_url_rejects_unsupported_scheme() {
		let url = Url::parse("ftp://example.com/").unwrap();
		let err = DataReaderHttp::try_from(&url).unwrap_err();
		assert!(err.to_string().contains("unsupported URL scheme"));
	}

	#[test]
	fn default_in_flight_cap() -> Result<()> {
		// Use a host unique to this test so other tests can't mutate its permits.
		let reader = DataReaderHttp::try_from(&Url::parse("https://default-cap-test.invalid/").unwrap())?;
		assert_eq!(reader.in_flight.available_permits(), DEFAULT_MAX_IN_FLIGHT);
		Ok(())
	}

	#[test]
	fn with_max_in_flight_overrides_cap() -> Result<()> {
		// Overriding installs a private semaphore; host-shared one is bypassed.
		let reader =
			DataReaderHttp::try_from(&Url::parse("https://override-cap-test.invalid/").unwrap())?.with_max_in_flight(8);
		assert_eq!(reader.in_flight.available_permits(), 8);
		Ok(())
	}

	#[test]
	fn with_max_in_flight_clamps_below_two() -> Result<()> {
		// split_and_read fans out to two halves; a cap of 1 would deadlock.
		let reader =
			DataReaderHttp::try_from(&Url::parse("https://clamp-low-test.invalid/").unwrap())?.with_max_in_flight(0);
		assert_eq!(reader.in_flight.available_permits(), 2);
		let reader =
			DataReaderHttp::try_from(&Url::parse("https://clamp-one-test.invalid/").unwrap())?.with_max_in_flight(1);
		assert_eq!(reader.in_flight.available_permits(), 2);
		Ok(())
	}

	#[test]
	fn semaphore_shared_across_readers_with_same_host() -> Result<()> {
		let r1 = DataReaderHttp::try_from(&Url::parse("https://shared-host-test.invalid/a.bin").unwrap())?;
		let r2 = DataReaderHttp::try_from(&Url::parse("https://shared-host-test.invalid/b.bin").unwrap())?;
		// Same Arc -> same underlying Semaphore instance.
		assert!(Arc::ptr_eq(&r1.in_flight, &r2.in_flight));
		Ok(())
	}

	#[test]
	fn semaphore_distinct_across_hosts() -> Result<()> {
		let r1 = DataReaderHttp::try_from(&Url::parse("https://host-a-test.invalid/").unwrap())?;
		let r2 = DataReaderHttp::try_from(&Url::parse("https://host-b-test.invalid/").unwrap())?;
		assert!(!Arc::ptr_eq(&r1.in_flight, &r2.in_flight));
		Ok(())
	}
}

#[cfg(test)]
mod body_limit_tests {
	use tokio::io::{AsyncReadExt, AsyncWriteExt};

	use super::*;

	/// Answers exactly one request with a fixed response, then stops.
	///
	/// Written by hand rather than with a mock library because the point is to
	/// send a body that disagrees with the headers describing it, which a
	/// well-behaved HTTP server will not do.
	async fn serve_once(response: Vec<u8>) -> Url {
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let addr = listener.local_addr().unwrap();

		tokio::spawn(async move {
			if let Ok((mut socket, _)) = listener.accept().await {
				let mut discard = [0u8; 2048];
				let _ = socket.read(&mut discard).await;
				let _ = socket.write_all(&response).await;
				let _ = socket.flush().await;
			}
		});

		Url::parse(&format!("http://{addr}/x")).unwrap()
	}

	fn partial_response(content_range: &str, body: &[u8]) -> Vec<u8> {
		let mut out = format!(
			"HTTP/1.1 206 Partial Content\r\nContent-Range: {content_range}\r\nContent-Length: {}\r\n\r\n",
			body.len()
		)
		.into_bytes();
		out.extend_from_slice(body);
		out
	}

	/// The `Content-Range` header was checked, but nothing constrained how many
	/// bytes followed it — so a server answering a small range request with a
	/// huge body was answered by buffering the whole thing.
	#[tokio::test]
	async fn a_body_longer_than_the_requested_range_is_refused() {
		let body = vec![b'x'; 5000];
		let url = serve_once(partial_response("bytes 0-9/100", &body)).await;
		let reader = DataReaderHttp::try_from(&url).unwrap();

		let error = reader
			.read_range(&ByteRange::new(0, 10))
			.await
			.expect_err("an over-long body must be refused");

		let message = format!("{error:#}");
		assert!(message.contains("longer than"), "unhelpful message: {message}");
	}

	/// A short body used to be concatenated into the result by `split_and_read`,
	/// which turned a truncated response into a blob that looked complete.
	#[tokio::test]
	async fn a_body_shorter_than_the_requested_range_is_refused() {
		let url = serve_once(partial_response("bytes 0-9/100", b"abc")).await;
		let reader = DataReaderHttp::try_from(&url).unwrap();

		let error = reader
			.read_range(&ByteRange::new(0, 10))
			.await
			.expect_err("a short body must be refused");

		let message = format!("{error:#}");
		assert!(
			message.contains("expected 10 bytes") && message.contains("carried 3"),
			"unhelpful message: {message}"
		);
	}

	/// A response that matches what was asked for still reads normally.
	#[tokio::test]
	async fn a_body_matching_the_requested_range_is_accepted() {
		let url = serve_once(partial_response("bytes 0-9/100", b"0123456789")).await;
		let reader = DataReaderHttp::try_from(&url).unwrap();

		let blob = reader.read_range(&ByteRange::new(0, 10)).await.unwrap();
		assert_eq!(blob.as_slice(), b"0123456789");
	}
}

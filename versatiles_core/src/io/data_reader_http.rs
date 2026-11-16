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
	fmt,
	path::PathBuf,
	str,
	sync::{Arc, LazyLock, OnceLock, atomic::AtomicU64},
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
	DataReaderTrait, http_credentials,
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

/// The most [`read_body_limited`] reserves up front on the peer's word.
const MAX_PREALLOCATED_BODY: u64 = 16 * 1024 * 1024;

/// Read a response body, refusing to buffer more than `limit` bytes.
///
/// `Response::bytes()` buffers whatever the peer sends: a server answering a
/// 64 KiB range request with ten gigabytes is answered by allocating ten
/// gigabytes, and `Content-Length` is the peer's claim rather than a
/// constraint. Reading the stream chunk by chunk with a running total stops at
/// the limit instead of after it. Each chunk's size goes to `on_bytes`.
async fn read_body_limited(
	response: reqwest::Response,
	limit: u64,
	on_bytes: &(dyn Fn(u64) + Sync),
) -> std::result::Result<Vec<u8>, BodyError> {
	use futures::StreamExt;

	// Only as a starting size, and only when the peer's claim is plausible —
	// it is not trusted as a bound. Capped as well: `limit` is the range length
	// asked for, which itself comes from the file being read, so a crafted
	// index naming gigabytes would otherwise reserve them before a byte
	// arrives. Past the cap the vector grows as the data does.
	let mut body = match response.content_length() {
		Some(len) if len <= limit => Vec::with_capacity(usize::try_from(len.min(MAX_PREALLOCATED_BODY)).unwrap_or(0)),
		_ => Vec::new(),
	};

	let mut stream = response.bytes_stream();
	while let Some(chunk) = stream.next().await {
		let chunk = chunk.map_err(BodyError::Transport)?;
		if (body.len() as u64).saturating_add(chunk.len() as u64) > limit {
			return Err(BodyError::TooLarge { limit });
		}
		body.extend_from_slice(&chunk);
		on_bytes(chunk.len() as u64);
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

	let expected_end = range.last()?;
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
	/// The URL as given, credentials included; only for [`DataReaderTrait::url`].
	given_url: Url,
	url: Url,
	username: Option<String>,
	/// The password written into the URL. Without one, [`Self::password`]
	/// looks one up for `username`.
	password: Option<String>,
	/// The `.netrc` file to look in, if reading it is enabled.
	netrc_file: Option<PathBuf>,
	/// What `netrc_file` has for `username`, read at the first request.
	netrc_password: OnceLock<Option<String>>,
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
		let given_url = url.clone();
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
			given_url,
			url,
			netrc_file: if username.is_some() && password.is_none() {
				http_credentials::netrc_file()
			} else {
				None
			},
			username,
			password,
			netrc_password: OnceLock::new(),
			max_request_bytes: AtomicU64::new(u64::MAX),
			in_flight,
		})
	}
}

impl DataReaderHttp {
	fn apply_auth(&self, builder: RequestBuilder) -> RequestBuilder {
		if let Some(username) = &self.username {
			builder.basic_auth(username, self.password())
		} else {
			builder
		}
	}

	/// The password sent with the URL's user.
	///
	/// The one written into the URL, or else the one registered with
	/// [`set_http_password`](super::set_http_password) for this server and
	/// user, or else the one in `.netrc` if reading it is enabled and the URL
	/// may carry it. Looked up for every request, so a password registered or
	/// forgotten after the reader was made counts from the next request on. A
	/// URL with no user has none.
	fn password(&self) -> Option<String> {
		if let Some(password) = &self.password {
			return Some(password.clone());
		}
		let user = self.username.as_deref()?;
		if let Some(password) = http_credentials::stored_password(&self.url, user) {
			return Some(password);
		}
		if !http_credentials::may_carry_a_netrc_password(&self.url) {
			return None;
		}
		self.netrc_entry()
	}

	/// What `.netrc` has for the URL's user on its host, read once, whether or
	/// not the URL may carry it.
	fn netrc_entry(&self) -> Option<String> {
		let file = self.netrc_file.as_deref()?;
		let user = self.username.as_deref()?;
		self
			.netrc_password
			.get_or_init(|| http_credentials::netrc_password(file, self.url.host_str().unwrap_or_default(), user))
			.clone()
	}

	/// What to try when a server answers `401` to a URL that names a user but
	/// no password could be found for.
	fn missing_password_hint(&self, status: StatusCode) -> Option<String> {
		if status != StatusCode::UNAUTHORIZED || self.password().is_some() {
			return None;
		}
		let user = self.username.as_deref()?;
		let host = self.url.host_str().unwrap_or_default();
		Some(match &self.netrc_file {
			Some(file) if self.netrc_entry().is_some() => format!(
				"the URL names the user '{user}' but gives no password. {} has one for it, but a password \
				 from .netrc is sent only over https, or over http to this machine: use https://{user}@{host}/…",
				file.display()
			),
			None => format!(
				"the URL names the user '{user}' but gives no password, and none is known for it. \
				 To take it from ~/.netrc, set VERSATILES_HTTP_NETRC=on, or pass --netrc on the command line"
			),
			Some(file) => format!(
				"the URL names the user '{user}' but gives no password, and {} has no entry \
				 'machine {host} login {user}'",
				file.display()
			),
		})
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
fn unexpected_status(
	range: &ByteRange,
	len: u64,
	url: &Url,
	status: StatusCode,
	total_attempts: u32,
	hint: Option<String>,
) -> anyhow::Error {
	if status.is_server_error() {
		return anyhow!(
			"could not read {range} ({len} bytes) from '{url}': server returned {status} — gave up after {total_attempts} attempts"
		);
	}

	let hint = hint.map_or_default(|hint| format!(": {hint}"));
	let error = anyhow!("could not read {range} ({len} bytes) from '{url}': expected HTTP 206, got {status}{hint}");
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
	/// Single-range read with retry/backoff, reporting each received chunk to `on_bytes`.
	async fn try_read_range_impl(&self, range: &ByteRange, on_bytes: &(dyn Fn(u64) + Sync)) -> Result<Blob> {
		// `last()` is checked: the range comes from a container header, and an
		// unchecked sum here panics in a debug build and asks for a wrapped byte
		// range in a release one. An empty range, which has no last byte, never
		// gets here: `network_read_range` answers it without a request.
		let request_range: String = format!("bytes={}-{}", range.offset, range.last()?);
		let policy = super::retry::policy();
		let max_retries = policy.max_retries;
		let total_attempts = max_retries.saturating_add(1);
		let url = &self.url;
		let len = range.length;

		for attempt in 0..=max_retries {
			let attempt_label = format!("attempt {}/{total_attempts}", attempt.saturating_add(1));

			if let Some(retry) = attempt.checked_sub(1) {
				let backoff = policy.backoff(retry);
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
				return Err(unexpected_status(
					range,
					len,
					url,
					status,
					total_attempts,
					self.missing_password_hint(status),
				));
			}

			verify_content_range(&response, range)?;

			// Bounded by the range that was asked for. `Content-Range` was checked
			// above, but that is only the header: nothing so far constrains how
			// many bytes actually follow it.
			let bytes = match read_body_limited(response, range.length, on_bytes).await {
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
	async fn try_read_range(&self, range: &ByteRange, on_bytes: &(dyn Fn(u64) + Sync)) -> Result<Blob> {
		self.try_read_range_impl(range, on_bytes).await
	}

	fn max_request_bytes(&self) -> &AtomicU64 {
		&self.max_request_bytes
	}
}

#[async_trait]
impl DataReaderTrait for DataReaderHttp {
	async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		self.network_read_range(range, &|_| {}).await
	}

	/// Reports each chunk of the response body as it arrives.
	async fn read_range_with_progress(&self, range: &ByteRange, on_bytes: &(dyn Fn(u64) + Sync)) -> Result<Blob> {
		self.network_read_range(range, on_bytes).await
	}

	/// Reads all the data from the HTTP(S) endpoint.
	///
	/// # Returns
	///
	/// * A Result containing a Blob with all the data or an error.
	async fn read_all(&self) -> Result<Blob> {
		let policy = super::retry::policy();
		let max_retries = policy.max_retries;
		let total_attempts = max_retries.saturating_add(1);
		let url = &self.url;

		for attempt in 0..=max_retries {
			let attempt_label = format!("attempt {}/{total_attempts}", attempt.saturating_add(1));

			if let Some(retry) = attempt.checked_sub(1) {
				let backoff = policy.backoff(retry);
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
				let hint = self
					.missing_password_hint(status)
					.map_or_default(|hint| format!(": {hint}"));
				bail!("could not read from '{url}': server returned {status}{hint}");
			}

			let limit = env_u64("VERSATILES_MAX_HTTP_BODY_BYTES", DEFAULT_MAX_BODY_BYTES);
			let bytes = match read_body_limited(response, limit, &|_| {}).await {
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

	fn url(&self) -> Option<&Url> {
		Some(&self.given_url)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The name is safe to print; the URL a relative file is resolved
	/// against keeps the credentials, so that file is read the same way.
	#[test]
	fn the_url_keeps_its_credentials_and_the_name_does_not() {
		let url = Url::parse("https://user:secret@example.org/pipelines/p.vpl").unwrap();
		let reader = DataReaderHttp::try_from(&url).unwrap();
		assert_eq!(reader.url(), Some(&url));
		assert!(!reader.name().contains("secret"), "got: {}", reader.name());
	}

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

	/// A large read reports the bytes as they arrive (#304), and together they
	/// add up to the range.
	#[tokio::test]
	async fn read_range_with_progress_reports_the_received_bytes() {
		let body: Vec<u8> = (0..100_000u32).map(|i| i.to_le_bytes()[0]).collect();
		let url = serve_once(partial_response("bytes 0-99999/200000", &body)).await;
		let reader = DataReaderHttp::try_from(&url).unwrap();

		let received = std::sync::atomic::AtomicU64::new(0);
		let reports = std::sync::atomic::AtomicU64::new(0);
		let blob = reader
			.read_range_with_progress(&ByteRange::new(0, 100_000), &|bytes| {
				received.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
				reports.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
			})
			.await
			.unwrap();

		assert_eq!(blob.as_slice(), body.as_slice());
		assert_eq!(received.load(std::sync::atomic::Ordering::Relaxed), 100_000);
		assert!(reports.load(std::sync::atomic::Ordering::Relaxed) >= 1);
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

	/// A range from a crafted header whose end overflows is refused before a
	/// request is sent, instead of panicking or asking for a wrapped range.
	#[tokio::test]
	async fn a_range_whose_end_overflows_is_refused() {
		let url = Url::parse("http://127.0.0.1:9/x").unwrap();
		let reader = DataReaderHttp::try_from(&url).unwrap();

		let error = reader
			.read_range(&ByteRange::new(u64::MAX - 1, 5))
			.await
			.expect_err("an overflowing range must be refused");

		let message = format!("{error:#}");
		assert!(message.contains("overflows u64"), "unhelpful message: {message}");
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

#[cfg(test)]
mod credential_tests {
	use tokio::{
		io::{AsyncReadExt, AsyncWriteExt},
		sync::oneshot,
	};

	use super::*;
	use crate::io::set_http_password;

	/// Answers one request with `response`, and tells what its `Authorization`
	/// header was, if it had one.
	async fn serve_once(response: &'static str) -> (Url, oneshot::Receiver<Option<String>>) {
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let addr = listener.local_addr().unwrap();
		let (sender, receiver) = oneshot::channel();

		tokio::spawn(async move {
			if let Ok((mut socket, _)) = listener.accept().await {
				let mut request = vec![0u8; 4096];
				let n = socket.read(&mut request).await.unwrap_or(0);
				let request = String::from_utf8_lossy(&request[..n]).into_owned();
				let authorization = request.lines().find_map(|line| {
					let (name, value) = line.split_once(':')?;
					name
						.eq_ignore_ascii_case("authorization")
						.then(|| value.trim().to_string())
				});
				let _ = sender.send(authorization);
				let _ = socket.write_all(response.as_bytes()).await;
				let _ = socket.flush().await;
			}
		});

		(Url::parse(&format!("http://{addr}/x")).unwrap(), receiver)
	}

	const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
	const UNAUTHORIZED: &str = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n";

	/// `user:password` as Basic auth sends it.
	fn basic(credentials: &str) -> Option<String> {
		let (user, password) = credentials.split_once(':').unwrap();
		let request = Client::new()
			.get("http://127.0.0.1/")
			.basic_auth(user, (!password.is_empty()).then_some(password))
			.build()
			.unwrap();
		Some(request.headers()["authorization"].to_str().unwrap().to_string())
	}

	fn with_user(url: &Url, userinfo: &str) -> Url {
		Url::parse(&url.as_str().replacen("http://", &format!("http://{userinfo}@"), 1)).unwrap()
	}

	/// The `Authorization` header a reader for `url` sends.
	async fn sent_authorization(reader: &DataReaderHttp, receiver: oneshot::Receiver<Option<String>>) -> Option<String> {
		reader.read_all().await.unwrap();
		receiver.await.unwrap()
	}

	fn netrc(text: &str) -> (tempfile::TempDir, PathBuf) {
		let dir = tempfile::tempdir().unwrap();
		let file = dir.path().join("netrc");
		std::fs::write(&file, text).unwrap();
		(dir, file)
	}

	/// The store belongs to the process and every server here is on 127.0.0.1,
	/// where a port can come round again, so each test has user names of its own.
	#[tokio::test]
	async fn a_registered_password_is_sent_to_its_server() {
		let (url, receiver) = serve_once(OK).await;
		set_http_password(&url, "reg-alice", "from-the-store").unwrap();

		let reader = DataReaderHttp::try_from(&with_user(&url, "reg-alice")).unwrap();
		assert_eq!(
			sent_authorization(&reader, receiver).await,
			basic("reg-alice:from-the-store")
		);
		assert!(!reader.name().contains("from-the-store"));
		assert!(!format!("{reader:?}").contains("from-the-store"));
	}

	#[tokio::test]
	async fn a_registered_password_is_not_sent_to_another_port_or_user() {
		let (registered, _) = serve_once(OK).await;
		set_http_password(&registered, "other-alice", "not-yours").unwrap();
		set_http_password(&registered, "other-bob", "not-yours").unwrap();

		let (url, receiver) = serve_once(OK).await;
		let reader = DataReaderHttp::try_from(&with_user(&url, "other-alice")).unwrap();
		assert_eq!(sent_authorization(&reader, receiver).await, basic("other-alice:"));

		let (url, receiver) = serve_once(OK).await;
		set_http_password(&url, "other-bob", "bobs").unwrap();
		let reader = DataReaderHttp::try_from(&with_user(&url, "other-alice")).unwrap();
		assert_eq!(sent_authorization(&reader, receiver).await, basic("other-alice:"));
	}

	#[tokio::test]
	async fn a_password_in_the_url_wins() {
		let (url, receiver) = serve_once(OK).await;
		set_http_password(&url, "wins-alice", "from-the-store").unwrap();
		let (_dir, file) = netrc("machine 127.0.0.1 login wins-alice password from-netrc\n");

		let mut reader = DataReaderHttp::try_from(&with_user(&url, "wins-alice:from-the-url")).unwrap();
		reader.netrc_file = Some(file);
		assert_eq!(
			sent_authorization(&reader, receiver).await,
			basic("wins-alice:from-the-url")
		);
	}

	#[tokio::test]
	async fn netrc_is_not_read_unless_enabled() {
		let (url, _) = serve_once(OK).await;
		let reader = DataReaderHttp::try_from(&with_user(&url, "plain-alice")).unwrap();
		// The tests never enable it, and `VERSATILES_HTTP_NETRC` is not set for them.
		assert_eq!(reader.netrc_file, None);
		assert_eq!(reader.password(), None);
	}

	#[tokio::test]
	async fn netrc_gives_the_password_of_the_urls_user() {
		let (_dir, file) = netrc(
			"machine 127.0.0.1 login netrc-bob password bobs\nmachine 127.0.0.1 login netrc-alice password alices\n",
		);

		let (url, receiver) = serve_once(OK).await;
		let mut reader = DataReaderHttp::try_from(&with_user(&url, "netrc-alice")).unwrap();
		reader.netrc_file = Some(file.clone());
		assert_eq!(sent_authorization(&reader, receiver).await, basic("netrc-alice:alices"));

		let (url, receiver) = serve_once(OK).await;
		let mut reader = DataReaderHttp::try_from(&with_user(&url, "netrc-carol")).unwrap();
		reader.netrc_file = Some(file.clone());
		assert_eq!(sent_authorization(&reader, receiver).await, basic("netrc-carol:"));

		// A URL with no user sends no credentials at all.
		let (url, receiver) = serve_once(OK).await;
		let mut reader = DataReaderHttp::try_from(&url).unwrap();
		reader.netrc_file = Some(file);
		assert_eq!(sent_authorization(&reader, receiver).await, None);
	}

	#[tokio::test]
	async fn the_store_comes_before_netrc() {
		let (_dir, file) = netrc("machine 127.0.0.1 login order-alice password from-netrc\n");
		let (url, receiver) = serve_once(OK).await;
		set_http_password(&url, "order-alice", "from-the-store").unwrap();

		let mut reader = DataReaderHttp::try_from(&with_user(&url, "order-alice")).unwrap();
		reader.netrc_file = Some(file);
		assert_eq!(
			sent_authorization(&reader, receiver).await,
			basic("order-alice:from-the-store")
		);
	}

	#[tokio::test]
	async fn a_401_without_a_password_names_the_setting() {
		let (url, _) = serve_once(UNAUTHORIZED).await;
		let reader = DataReaderHttp::try_from(&with_user(&url, "hint-alice")).unwrap();
		let message = format!("{:#}", reader.read_all().await.unwrap_err());
		assert!(
			message.contains("401") && message.contains("VERSATILES_HTTP_NETRC=on") && message.contains("--netrc"),
			"got: {message}"
		);

		// A range read says the same.
		let (url, _) = serve_once(UNAUTHORIZED).await;
		let reader = DataReaderHttp::try_from(&with_user(&url, "hint-alice")).unwrap();
		let message = format!("{:#}", reader.read_range(&ByteRange::new(0, 10)).await.unwrap_err());
		assert!(message.contains("VERSATILES_HTTP_NETRC=on"), "got: {message}");
	}

	#[tokio::test]
	async fn a_401_with_netrc_enabled_names_the_entry_it_lacked() {
		let (_dir, file) = netrc("machine 127.0.0.1 login hintnetrc-bob password bobs\n");
		let (url, _) = serve_once(UNAUTHORIZED).await;
		let mut reader = DataReaderHttp::try_from(&with_user(&url, "hintnetrc-alice")).unwrap();
		reader.netrc_file = Some(file);
		let message = format!("{:#}", reader.read_all().await.unwrap_err());
		assert!(
			message.contains("'machine 127.0.0.1 login hintnetrc-alice'"),
			"got: {message}"
		);
	}

	/// Over plain http the password would cross the network for anyone to read,
	/// and a shared pipeline can name `http://` for a host `.netrc` has.
	#[test]
	fn netrc_is_not_sent_over_plain_http_to_another_machine() {
		let (_dir, file) = netrc("machine plain-http.invalid login plainhttp-alice password secret\n");

		let url = Url::parse("http://plainhttp-alice@plain-http.invalid/x.pmtiles").unwrap();
		let mut reader = DataReaderHttp::try_from(&url).unwrap();
		reader.netrc_file = Some(file.clone());
		assert_eq!(reader.password(), None);
		let hint = reader.missing_password_hint(StatusCode::UNAUTHORIZED).unwrap();
		assert!(
			hint.contains("sent only over https") && hint.contains("https://plainhttp-alice@plain-http.invalid/"),
			"got: {hint}"
		);
		assert!(!hint.contains("secret"), "password leaked: {hint}");

		let url = Url::parse("https://plainhttp-alice@plain-http.invalid/x.pmtiles").unwrap();
		let mut reader = DataReaderHttp::try_from(&url).unwrap();
		reader.netrc_file = Some(file);
		assert_eq!(reader.password().as_deref(), Some("secret"));
	}

	#[tokio::test]
	async fn a_401_with_a_password_or_without_a_user_has_no_hint() {
		for userinfo in [Some("nohint-alice:wrong"), None] {
			let (url, _) = serve_once(UNAUTHORIZED).await;
			let url = userinfo.map_or(url.clone(), |userinfo| with_user(&url, userinfo));
			let reader = DataReaderHttp::try_from(&url).unwrap();
			let message = format!("{:#}", reader.read_all().await.unwrap_err());
			assert!(message.contains("401"), "got: {message}");
			assert!(!message.contains("NETRC"), "got: {message}");
			assert!(!message.contains("wrong"), "password leaked: {message}");
		}
	}
}

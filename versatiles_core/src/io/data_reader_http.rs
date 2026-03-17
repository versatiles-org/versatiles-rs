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

use super::DataReaderTrait;
use crate::{Blob, ByteRange};
use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use percent_encoding::percent_decode_str;
use regex::{Regex, RegexBuilder};
use reqwest::{Client, RequestBuilder, StatusCode, Url};
use std::{fmt, str, sync::LazyLock, time::Duration};
use tokio::time::sleep;
use versatiles_derive::context;

/// A struct that provides reading capabilities from an HTTP(S) endpoint.
pub struct DataReaderHttp {
	client: Client,
	name: String,
	url: Url,
	username: Option<String>,
	password: Option<String>,
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
			.tcp_keepalive(Duration::from_secs(600))
			.use_rustls_tls()
			.build()?;

		Ok(DataReaderHttp {
			client,
			name: url.to_string(),
			url,
			username,
			password,
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
}

const MAX_RETRIES: u32 = 3;

fn is_retryable_error(err: &reqwest::Error) -> bool {
	err.is_connect() || err.is_timeout() || err.is_body()
}

#[async_trait]
impl DataReaderTrait for DataReaderHttp {
	/// Reads a specific range of bytes from the HTTP(S) endpoint.
	///
	/// # Arguments
	///
	/// * `range` - A `ByteRange` struct specifying the offset and length of the range to read.
	///
	/// # Returns
	///
	/// * A Result containing a Blob with the read data or an error.
	#[context("reading range {} from '{}'", range, self.url)]
	async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		let request_range: String = format!("bytes={}-{}", range.offset, range.length + range.offset - 1);

		for attempt in 0..=MAX_RETRIES {
			if attempt > 0 {
				let backoff = Duration::from_secs(1 << (attempt - 1));
				log::warn!(
					"retry attempt {attempt}/{MAX_RETRIES} reading range {range} from '{}', waiting {backoff:?}",
					self.url
				);
				sleep(backoff).await;
			}

			let response = match self
				.apply_auth(self.client.get(self.url.clone()))
				.header("range", &request_range)
				.send()
				.await
			{
				Ok(r) => r,
				Err(e) if is_retryable_error(&e) && attempt < MAX_RETRIES => {
					log::warn!("retryable error: {e}");
					continue;
				}
				Err(e) => return Err(e.into()),
			};

			if response.status() != StatusCode::PARTIAL_CONTENT {
				bail!("expected HTTP 206 (Partial Content), got {}", response.status());
			}

			let content_range = response
				.headers()
				.get("content-range")
				.ok_or_else(|| anyhow!("response is missing Content-Range header"))?
				.to_str()?;

			static RE_RANGE: LazyLock<Regex> = LazyLock::new(|| {
				RegexBuilder::new(r"^bytes (\d+)-(\d+)/\d+$")
					.case_insensitive(true)
					.build()
					.unwrap()
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

			let expected_end = range.offset + range.length - 1;
			if content_range_end != expected_end {
				bail!("Content-Range end mismatch: expected {expected_end}, got {content_range_end}");
			}

			let bytes = match response.bytes().await {
				Ok(b) => b,
				Err(e) if is_retryable_error(&e) && attempt < MAX_RETRIES => {
					log::warn!("retryable error reading response body: {e}");
					continue;
				}
				Err(e) => return Err(e.into()),
			};

			return Ok(Blob::from(&*bytes));
		}

		bail!("request failed after {MAX_RETRIES} retries")
	}

	/// Reads all the data from the HTTP(S) endpoint.
	///
	/// # Returns
	///
	/// * A Result containing a Blob with all the data or an error.
	#[context("reading all data from '{}'", self.url)]
	async fn read_all(&self) -> Result<Blob> {
		for attempt in 0..=MAX_RETRIES {
			if attempt > 0 {
				let backoff = Duration::from_secs(1 << (attempt - 1));
				log::warn!(
					"retry attempt {attempt}/{MAX_RETRIES} reading from '{}', waiting {backoff:?}",
					self.url
				);
				sleep(backoff).await;
			}

			let response = match self.apply_auth(self.client.get(self.url.clone())).send().await {
				Ok(r) => r,
				Err(e) if is_retryable_error(&e) && attempt < MAX_RETRIES => {
					log::warn!("retryable error: {e}");
					continue;
				}
				Err(e) => return Err(e.into()),
			};

			if !response.status().is_success() {
				bail!("HTTP request failed with status {}", response.status());
			}

			let bytes = match response.bytes().await {
				Ok(b) => b,
				Err(e) if is_retryable_error(&e) && attempt < MAX_RETRIES => {
					log::warn!("retryable error reading response body: {e}");
					continue;
				}
				Err(e) => return Err(e.into()),
			};

			return Ok(Blob::from(&*bytes));
		}

		bail!("request failed after {MAX_RETRIES} retries")
	}

	/// Gets the name of the data source.
	///
	/// # Returns
	///
	/// * A string slice representing the name of the data source.
	fn get_name(&self) -> &str {
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
		assert_eq!(data_reader_http.get_name(), url);

		Ok(())
	}

	#[test]
	fn from_url_with_credentials() -> Result<()> {
		let url = Url::parse("https://user:p%40ss@example.com/data.bin").unwrap();
		let reader = DataReaderHttp::try_from(&url)?;

		assert_eq!(reader.username.as_deref(), Some("user"));
		assert_eq!(reader.password.as_deref(), Some("p@ss"));
		assert_eq!(reader.get_name(), "https://example.com/data.bin");
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
		assert_eq!(reader.get_name(), "https://example.com/data.bin");

		Ok(())
	}
}

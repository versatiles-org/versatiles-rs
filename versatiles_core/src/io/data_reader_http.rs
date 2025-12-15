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
//!     let mut reader = DataReaderHttp::from_url(url)?;
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
use lazy_static::lazy_static;
use regex::{Regex, RegexBuilder};
use reqwest::{Client, Method, Request, StatusCode, Url};
use std::{str, time::Duration};
use versatiles_derive::context;

/// A struct that provides reading capabilities from an HTTP(S) endpoint.
#[derive(Debug)]
pub struct DataReaderHttp {
	client: Client,
	name: String,
	url: Url,
}

impl DataReaderHttp {
	/// Creates a `DataReaderHttp` from a URL.
	///
	/// # Arguments
	///
	/// * `url` - The URL of the HTTP(S) endpoint.
	///
	/// # Returns
	///
	/// * A Result containing a boxed `DataReaderHttp` or an error.
	pub fn from_url(url: Url) -> Result<Box<DataReaderHttp>> {
		match url.scheme() {
			"http" | "https" => (),
			_ => bail!("url has wrong scheme {url}"),
		}

		let client = Client::builder()
			.tcp_keepalive(Duration::from_secs(600))
			.connection_verbose(true)
			.danger_accept_invalid_certs(true)
			.use_rustls_tls()
			.build()?;

		Ok(Box::new(DataReaderHttp {
			client,
			name: url.to_string(),
			url,
		}))
	}
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
	#[context("while reading range {} from url '{}'", range, self.url)]
	async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		let ctx = || format!("while reading range {range} of {}", self.url);

		let mut request = Request::new(Method::GET, self.url.clone());
		let request_range: String = format!("bytes={}-{}", range.offset, range.length + range.offset - 1);
		request
			.headers_mut()
			.append("range", request_range.parse().with_context(ctx)?);

		let response = self.client.execute(request).await.with_context(ctx)?;

		if response.status() != StatusCode::PARTIAL_CONTENT {
			let status_code = response.status();
			bail!(
				"expected 206 as a response to a range request. instead we got {status_code}, {}",
				ctx()
			);
		}

		let content_range: &str = match response.headers().get("content-range") {
			Some(header_value) => header_value.to_str().with_context(ctx)?,
			None => bail!("content-range header is not set in response headers, {}", ctx()),
		};

		lazy_static! {
			static ref RE_RANGE: Regex = RegexBuilder::new(r"^bytes (\d+)-(\d+)/\d+$")
				.case_insensitive(true)
				.build()
				.unwrap();
		}

		// Extract "start" and "end" numbers from the Content‑Range header
		let (content_range_start, content_range_end) = {
			let caps = RE_RANGE
				.captures(content_range)
				.ok_or_else(|| anyhow!("invalid content-range header: {content_range}"))
				.with_context(ctx)?;
			(
				caps[1].parse::<u64>().with_context(ctx)?,
				caps[2].parse::<u64>().with_context(ctx)?,
			)
		};

		if content_range_start != range.offset {
			bail!(
				"content-range-start {content_range_start} is not start of range, {}",
				ctx()
			);
		}

		if content_range_end != range.offset + range.length - 1 {
			bail!("content-range-end {content_range_end} is not end of range, {}", ctx());
		}

		let bytes = response.bytes().await.with_context(ctx)?;

		Ok(Blob::from(&*bytes))

		//.with_context(|| format!("while reading {} (range {range_val})", self.url))
	}

	/// Reads all the data from the HTTP(S) endpoint.
	///
	/// # Returns
	///
	/// * A Result containing a Blob with all the data or an error.
	#[context("while reading all data from url '{}'", self.url)]
	async fn read_all(&self) -> Result<Blob> {
		let ctx = || format!("while reading all data from {}", self.url);
		let response = self.client.get(self.url.clone()).send().await.with_context(ctx)?;
		if !response.status().is_success() {
			let status = response.status();
			bail!("expected successful response, got {status}, {}", ctx());
		}
		let bytes = response.bytes().await.with_context(ctx)?;
		Ok(Blob::from(&*bytes))
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
		let data_reader_http = DataReaderHttp::from_url(valid_url);
		assert!(data_reader_http.is_ok());

		// Test with an invalid URL
		let data_reader_http = DataReaderHttp::from_url(invalid_url);
		assert!(data_reader_http.is_err());
	}

	async fn read_range_helper(url: &str, offset: u64, length: u64, expected: &str) -> Result<()> {
		let url = Url::parse(url).unwrap();
		let data_reader_http = DataReaderHttp::from_url(url)?;

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
			"https://raw.githubusercontent.com/versatiles-org/versatiles-rs/main/testdata/berlin.mbtiles",
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
		let data_reader_http = DataReaderHttp::from_url(Url::parse(url).unwrap())?;

		// Check if the name matches the original URL
		assert_eq!(data_reader_http.get_name(), url);

		Ok(())
	}
}

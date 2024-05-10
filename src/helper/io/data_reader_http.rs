use super::DataReaderTrait;
use crate::types::{Blob, ByteRange};
use anyhow::{bail, Result};
use async_trait::async_trait;
use lazy_static::lazy_static;
use log::info;
use regex::{Regex, RegexBuilder};
use reqwest::{Client, Method, Request, StatusCode, Url};
use std::str;
use std::time::Duration;

#[derive(Debug)]
pub struct DataReaderHttp {
	name: String,
	url: Url,
	client: Client,
}
impl DataReaderHttp {
	pub fn new(url: Url) -> Result<Box<Self>> {
		match url.scheme() {
			"http" => (),
			"https" => (),
			_ => bail!("url has wrong scheme {url}"),
		}

		let client = reqwest::Client::builder()
			.tcp_keepalive(Duration::from_secs(600))
			.connection_verbose(true)
			.danger_accept_invalid_certs(true)
			.use_rustls_tls()
			.build()?;

		Ok(Box::new(Self {
			name: url.to_string(),
			url,
			client,
		}))
	}
}
#[async_trait]
impl DataReaderTrait for DataReaderHttp {
	async fn read_range(&mut self, range: &ByteRange) -> Result<Blob> {
		let mut request = Request::new(Method::GET, self.url.clone());
		let request_range: String = format!("bytes={}-{}", range.offset, range.length + range.offset - 1);
		request.headers_mut().append("range", request_range.parse()?);

		let response = self.client.execute(request).await?;

		if response.status() != StatusCode::PARTIAL_CONTENT {
			let status_code = response.status();
			info!("response: {}", str::from_utf8(&response.bytes().await?)?);
			bail!(
				"as a response to a range request it is expected to get the status code 206. instead we got {status_code}"
			);
		}

		let content_range: &str = match response.headers().get("content-range") {
			Some(header_value) => header_value.to_str()?,
			None => bail!(
				"content-range is not set for range request {range:?} to url {}",
				self.url
			),
		};

		lazy_static! {
			static ref RE_RANGE: Regex = RegexBuilder::new(r"^bytes (\d+)-(\d+)/\d+$")
				.case_insensitive(true)
				.build()
				.unwrap();
		}

		let content_range_start: u64;
		let content_range_end: u64;
		if let Some(captures) = RE_RANGE.captures(content_range) {
			content_range_start = captures.get(1).unwrap().as_str().parse::<u64>()?;
			content_range_end = captures.get(2).unwrap().as_str().parse::<u64>()?;
		} else {
			bail!("format of content-range response is invalid: {content_range}");
		}

		if content_range_start != range.offset {
			bail!("content-range-start {content_range_start} is not start of range {range:?}");
		}

		if content_range_end != range.offset + range.length - 1 {
			bail!("content-range-end {content_range_end} is not end of range {range:?}");
		}

		let bytes = response.bytes().await?;

		Ok(Blob::from(bytes))
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;
	use reqwest::Url;
	use std::str;

	// Test the 'new' method for valid and invalid URLs
	#[tokio::test]
	async fn new() {
		let valid_url = Url::parse("https://www.example.com").unwrap();
		let invalid_url = Url::parse("ftp://www.example.com").unwrap();

		// Test with a valid URL
		let data_reader_http = DataReaderHttp::new(valid_url);
		assert!(data_reader_http.is_ok());

		// Test with an invalid URL
		let data_reader_http = DataReaderHttp::new(invalid_url);
		assert!(data_reader_http.is_err());
	}
	async fn read_range_helper(url: &str, offset: u64, length: u64, expected: &str) -> Result<()> {
		let url = Url::parse(url).unwrap();
		let mut data_reader_http = DataReaderHttp::new(url)?;

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
		.unwrap()
	}

	#[tokio::test]
	async fn read_range_googleapis() {
		read_range_helper(
			"https://storage.googleapis.com/versatiles/download/planet/planet-20230529.versatiles",
			3,
			12,
			"satiles_v02 ",
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
	#[tokio::test]
	async fn get_name() -> Result<()> {
		let url = "https://www.example.com/";
		let data_reader_http = DataReaderHttp::new(Url::parse(url).unwrap())?;

		// Check if the name matches the original URL
		assert_eq!(data_reader_http.get_name(), url);

		Ok(())
	}
}

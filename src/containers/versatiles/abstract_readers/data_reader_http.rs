use super::super::types::ByteRange;
use super::DataReaderTrait;
use crate::create_error;
use crate::shared::{Blob, Result};
use async_trait::async_trait;
use lazy_static::lazy_static;
use regex::{Regex, RegexBuilder};
use reqwest::{Client, Method, Request, StatusCode, Url};
use std::str;
use std::time::Duration;

pub struct DataReaderHttp {
	name: String,
	url: Url,
	client: Client,
}

#[async_trait]
impl DataReaderTrait for DataReaderHttp {
	async fn new(source: &str) -> Result<Box<Self>> {
		if source.starts_with("https://") || source.starts_with("http://") {
			let client = reqwest::Client::builder()
				.tcp_keepalive(Duration::from_secs(600))
				.connection_verbose(true)
				.danger_accept_invalid_certs(true)
				.use_rustls_tls()
				.build()?;
			Ok(Box::new(Self {
				name: source.to_string(),
				url: Url::parse(source)?,
				client,
			}))
		} else {
			create_error!("source {source} must start with http:// or https://")
		}
	}
	async fn read_range(&mut self, range: &ByteRange) -> Result<Blob> {
		let mut request = Request::new(Method::GET, self.url.clone());
		let request_range: String = format!("bytes={}-{}", range.offset, range.length + range.offset - 1);
		request.headers_mut().append("range", request_range.parse()?);

		let response = self.client.execute(request).await?;

		if response.status() != StatusCode::PARTIAL_CONTENT {
			let status_code = response.status();
			println!("response: {}", str::from_utf8(&response.bytes().await?)?);
			return create_error!(
				"as a response to a range request it is expected to get the status code 206. instead we got {status_code}"
			);
		}

		let content_range: &str;
		match response.headers().get("content-range") {
			Some(header_value) => content_range = header_value.to_str()?,
			None => {
				return create_error!(
					"content-range is not set for range request {range:?} to url {}",
					self.url
				)
			}
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
			return create_error!("format of content-range response is invalid: {content_range}");
		}

		if content_range_start != range.offset {
			return create_error!("content-range-start {content_range_start} is not start of range {range:?}");
		}

		if content_range_end != range.offset + range.length - 1 {
			return create_error!("content-range-end {content_range_end} is not end of range {range:?}");
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
	use super::{DataReaderHttp, DataReaderTrait};
	use crate::{containers::versatiles::types::ByteRange, shared::Result};
	use std::str;

	// Test the 'new' method for valid and invalid URLs
	#[tokio::test]
	async fn new() {
		let valid_url = "https://www.example.com";
		let invalid_url = "ftp://www.example.com";

		// Test with a valid URL
		let data_reader_http = DataReaderHttp::new(valid_url).await;
		assert!(data_reader_http.is_ok());

		// Test with an invalid URL
		let data_reader_http = DataReaderHttp::new(invalid_url).await;
		assert!(data_reader_http.is_err());
	}

	// Test the 'read_range' method
	#[tokio::test]
	async fn read_range() -> Result<()> {
		async fn test(url: &str, check: (u64, u64, &str)) -> Result<()> {
			let mut data_reader_http = DataReaderHttp::new(url).await?;

			// Define a range to read
			let range = ByteRange {
				offset: check.0,
				length: check.1,
			};

			// Read the specified range from the URL
			let blob = data_reader_http.read_range(&range).await?;

			// Convert the resulting Blob to a string
			let result_text = str::from_utf8(blob.as_slice())?;

			// Check if the read range matches the expected text
			assert_eq!(result_text, check.2);

			Ok(())
		}

		test(
			"https://raw.githubusercontent.com/versatiles-org/versatiles-rs/main/testdata/berlin.mbtiles",
			(7, 8, "format 3"),
		)
		.await?;

		test(
			"https://storage.googleapis.com/versatiles/download/planet/planet-20230529.versatiles",
			(3, 12, "satiles_v02 "),
		)
		.await?;

		test("https://google.com/", (100, 110, "plingplong")).await.unwrap_err();

		Ok(())
	}

	// Test the 'get_name' method
	#[tokio::test]
	async fn get_name() -> Result<()> {
		let url = "https://www.example.com";
		let data_reader_http = DataReaderHttp::new(url).await?;

		// Check if the name matches the original URL
		assert_eq!(data_reader_http.get_name(), url);

		Ok(())
	}
}

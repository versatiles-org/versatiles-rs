use super::super::types::ByteRange;
use super::DataReaderTrait;
use crate::shared::{Blob, Error, Result};
use async_trait::async_trait;
use lazy_static::lazy_static;
use regex::{Regex, RegexBuilder};
use reqwest::{Client, Method, Request, Url};
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
			Err(Error::new(&format!(
				"source {} must start with http:// or https://",
				source
			)))
		}
	}
	async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		let mut request = Request::new(Method::GET, self.url.clone());
		let request_range: String = format!("bytes={}-{}", range.offset, range.length + range.offset - 1);
		request.headers_mut().append("range", request_range.parse()?);

		let response = self.client.execute(request).await?;

		let content_length: u64 = match response.headers().get("content-length") {
			Some(text) => text.to_str()?.parse::<u64>()?,
			None => panic!("content-length not set for range request {range:?} to url {}", self.url),
		};

		let content_range: &str = match response.headers().get("content-range") {
			Some(header_value) => header_value.to_str()?,
			None => panic!(
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
			panic!("format of content-range response is invalid: {content_range}")
		}
		let content_range_length = content_range_end - content_range_start + 1;

		if content_range_length != content_length {
			panic!("content-range length {content_range_length} is not content-length {content_length}");
		}

		if content_range_start != range.offset {
			panic!("content-range-start {content_range_start} is not start of range {range:?}");
		}

		if content_range_end != range.offset + range.length - 1 {
			panic!("content-range-end {content_range_end} is not end of range {range:?}");
		}

		let bytes = response.bytes().await?;

		Ok(Blob::from(bytes))
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}

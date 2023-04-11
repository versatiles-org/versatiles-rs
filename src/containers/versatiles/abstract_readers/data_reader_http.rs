use super::super::types::ByteRange;
use super::DataReaderTrait;
use crate::shared::{Blob, Error, Result};
use async_trait::async_trait;
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

		//println!("### request:\n{:#?}", request);
		let result = self.client.execute(request).await?;
		//println!("### result:\n{:#?}", result);

		let bytes = result.bytes().await?;

		//let range = result.headers().get("content-range");
		//println!("range {:#?}", range);

		Ok(Blob::from(bytes))
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}

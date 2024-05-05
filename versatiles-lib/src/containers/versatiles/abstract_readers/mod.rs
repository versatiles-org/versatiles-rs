mod data_reader_file;
#[cfg(feature = "full")]
mod data_reader_http;
mod traits;

use anyhow::Context;
pub use data_reader_file::*;
#[cfg(feature = "full")]
pub use data_reader_http::*;
pub use traits::*;

use anyhow::Result;

pub async fn new_data_reader(url: &str) -> Result<Box<dyn DataReaderTrait>> {
	let start = url.split_terminator(':').next();

	Ok(match start {
		#[cfg(feature = "full")]
		Some("http" | "https") => DataReaderHttp::new(url).await.with_context(|| format!("opening {url} as http"))?,
		_ => DataReaderFile::new(url).await.with_context(|| format!("opening {url} as file"))?,
	})
}

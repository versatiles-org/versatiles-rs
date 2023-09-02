mod data_reader_file;
#[cfg(feature = "request")]
mod data_reader_http;
mod traits;

pub use data_reader_file::*;
#[cfg(feature = "request")]
pub use data_reader_http::*;
pub use traits::*;

use crate::shared::Result;

pub async fn new_data_reader(source: &str) -> Result<Box<dyn DataReaderTrait>> {
	let start = source.split_terminator(':').next();

	Ok(match start {
		#[cfg(feature = "request")]
		Some("http" | "https") => DataReaderHttp::new(source).await?,
		_ => DataReaderFile::new(source).await?,
	})
}

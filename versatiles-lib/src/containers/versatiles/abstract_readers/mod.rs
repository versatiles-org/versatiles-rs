mod data_reader_file;
#[cfg(feature = "full")]
mod data_reader_http;
mod traits;

pub use data_reader_file::*;
#[cfg(feature = "full")]
pub use data_reader_http::*;
pub use traits::*;

use crate::shared::Result;

pub async fn new_data_reader(source: &str) -> Result<Box<dyn DataReaderTrait>> {
	let start = source.split_terminator(':').next();

	Ok(match start {
		#[cfg(feature = "full")]
		Some("http" | "https") => DataReaderHttp::new(source).await?,
		_ => DataReaderFile::new(source).await?,
	})
}

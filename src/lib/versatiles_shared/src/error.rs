// use http::header::InvalidHeaderValue as http_header_InvalidHeaderValue;
// use reqwest::Error as reqwest_Error;
// use std::fmt::Error as std_fmt_Error;
// use std::io::Error as std_io_Error;
// use url::ParseError as url_ParseError;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone)]
pub struct Error {
	msg: String,
}

impl Error {
	pub fn new(msg: String) -> Box<Self> {
		Box::new(Self { msg })
	}
}

impl std::fmt::Display for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(f, "{}", self.msg)
	}
}

impl std::error::Error for Error {}

/*
impl std::fmt::Debug for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Error").finish()
	}
}

impl std::fmt::Display for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.msg)
	}
}

impl std::error::Error for Error {}
 */

pub struct Error {
	msg: String,
}

impl Error {
	pub fn new(msg: String) -> Self {
		Error { msg }
	}
}

impl From<std::io::Error> for Error {
	fn from(error: std::io::Error) -> Self {
		Self { msg: error.to_string() }
	}
}

impl From<std::fmt::Error> for Error {
	fn from(error: std::fmt::Error) -> Self {
		Self { msg: error.to_string() }
	}
}

impl From<http::header::InvalidHeaderValue> for Error {
	fn from(error: http::header::InvalidHeaderValue) -> Self {
		Self { msg: error.to_string() }
	}
}

impl From<reqwest::Error> for Error {
	fn from(error: reqwest::Error) -> Self {
		Self { msg: error.to_string() }
	}
}

impl From<url::ParseError> for Error {
	fn from(error: url::ParseError) -> Self {
		Self { msg: error.to_string() }
	}
}

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

/// A type alias for `std::result::Result` that uses `Box<dyn std::error::Error>` as the error type.
pub type Result<T> = std::result::Result<T, Error>;

/// Represents an error in the application.
#[derive(Debug, Clone)]
pub struct Error {
	msg: String,
}

impl Error {
	/// Creates a new `Error` instance with the given message.
	pub fn new(msg: &str) -> Self {
		Self { msg: msg.to_owned() }
	}
}

unsafe impl Send for Error {}

impl std::fmt::Display for Error {
	/// Formats the error message for display.
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(f, "{}", self.msg)
	}
}

impl<T: std::error::Error> From<T> for Error {
	fn from(error: T) -> Self {
		Self { msg: error.to_string() }
	}
}

#[macro_export]
macro_rules! create_error {
	($($arg:tt)*) => {
		Err($crate::shared::Error::new(&format!($($arg)*)))
	};
}

#[cfg(test)]
mod tests {
	use super::Error;

	#[test]
	fn test() {
		let err = Error::new("hi");
		let err = err.clone();
		format!("{}", err);
		format!("{:?}", err);
	}
}

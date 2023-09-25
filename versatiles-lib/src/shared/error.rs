use std::backtrace::Backtrace;

use colored::Colorize;

/// A type alias for `std::result::Result` that uses `Box<dyn std::error::Error>` as the error type.
pub type Result<T> = std::result::Result<T, Error>;

/// Represents an error in the application.
#[derive(Clone)]
pub struct Error {
	msg: String,
	backtrace: String,
}

impl Error {
	/// Creates a new `Error` instance with the given message.
	pub fn new(msg: String) -> Self {
		Self {
			msg,
			backtrace: format!("{}", Backtrace::force_capture()),
		}
	}
}

unsafe impl Send for Error {}

impl std::fmt::Display for Error {
	/// Formats the error message for display.
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		#[cfg(feature = "full")]
		return write!(f, "{}\n{}", self.msg.red().bold(), self.backtrace.dimmed());

		#[cfg(not(feature = "full"))]
		return write!(f, "{}\n{}", self.msg, self.backtrace);
	}
}

impl std::fmt::Debug for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self)
	}
}

impl<T: std::error::Error> From<T> for Error {
	fn from(error: T) -> Self {
		Self::new(error.to_string())
	}
}

#[macro_export]
macro_rules! create_error {
	($($arg:tt)*) => {
		Err($crate::shared::Error::new(format!($($arg)*)))
	};
}

#[cfg(test)]
mod tests {
	use super::Error;

	#[test]
	fn test() {
		let err = Error::new(String::from("hi"));
		let err = err.clone();
		format!("{}", err);
		format!("{:?}", err);
	}
}

/// A type alias for `std::result::Result` that uses `Box<dyn std::error::Error>` as the error type.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Represents an error in the application.
#[derive(Debug, Clone)]
pub struct Error {
	msg: String,
}

impl Error {
	/// Creates a new `Error` instance with the given message.
	pub fn new(msg: &str) -> Box<Self> {
		Box::new(Self { msg: msg.to_owned() })
	}
}

impl std::fmt::Display for Error {
	/// Formats the error message for display.
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(f, "{}", self.msg)
	}
}

impl std::error::Error for Error {}

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

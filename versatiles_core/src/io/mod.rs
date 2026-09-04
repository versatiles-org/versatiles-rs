//! This module re-exports all the data reader and writer modules, along with the value reader and writer modules.
//!
//! # Overview
//!
//! The module provides a unified interface for importing all the necessary components for reading and writing data
//! in various formats and from various sources. It includes readers and writers for blobs, files, HTTP sources (if enabled),
//! and more. The value readers and writers support different byte orders and offer functionality for handling various data types.
//!
//! # Examples
//!
//! ```rust
//! // Importing all the necessary components
//! use versatiles_core::io::*;
//!
//! fn main() {
//!     // Now you can use all the imported modules and structs, such as `DataReaderBlob`, `DataWriterFile`, etc.
//! }
//! ```

use std::sync::OnceLock;

use anyhow::{Result, ensure};

/// The VersaTiles half of the `User-Agent` header, fixed at compile time.
///
/// It identifies the software, its version, and an info URL so tile and data
/// providers can recognize (and, if needed, contact about) VersaTiles traffic,
/// e.g. `versatiles/4.1.5 (+https://versatiles.org)`.
///
/// This is the *base* of the header, not the whole of it: an application
/// embedding the library can append its own product token with
/// [`set_product`]. Read [`user_agent`] to get what is actually sent.
pub const USER_AGENT: &str = concat!("versatiles/", env!("CARGO_PKG_VERSION"), " (+https://versatiles.org)");

/// The full header once a product token has been added; unset until then.
static USER_AGENT_WITH_PRODUCT: OnceLock<String> = OnceLock::new();

/// Adds a product token identifying the host application to every request.
///
/// [`USER_AGENT`] answers "which software is this" with `versatiles/…`, which
/// is enough for the CLI and only half an answer for anything embedding the
/// library: every remote container VersaTiles Studio opens looks like
/// `versatiles convert` in a provider's log. This appends a second token, so
/// the header names both.
///
/// Appended, never substituted — RFC 9110 defines `User-Agent` as a list of
/// product tokens, and `versatiles/…` staying first is what keeps everything
/// that recognizes VersaTiles traffic today still working.
///
/// **The first call wins.** The identity belongs to the process, so a later
/// call is ignored rather than honoured: an application whose identity changes
/// halfway through a run is worse than one that ignores a second call.
///
/// `name` and `version` are separate because a product token is `name/version`;
/// passing them apart is what lets this reject a `name` with a space in it
/// rather than emit a header that parses as two products.
///
/// # Errors
///
/// If `name` or `version` is not a valid RFC 9110 token, or `url` contains a
/// space, a control character or a `)` that would close the comment early.
///
/// # Examples
///
/// ```
/// use versatiles_core::io::{set_product, user_agent};
///
/// set_product("VersaTiles-Studio", "0.4.0", Some("https://versatiles.org/studio")).unwrap();
/// assert!(user_agent().starts_with("versatiles/"));
/// assert!(user_agent().ends_with("VersaTiles-Studio/0.4.0 (+https://versatiles.org/studio)"));
/// ```
pub fn set_product(name: &str, version: &str, url: Option<&str>) -> Result<()> {
	ensure!(is_token(name), "product name {name:?} is not a valid User-Agent token");
	ensure!(
		is_token(version),
		"product version {version:?} is not a valid User-Agent token"
	);
	if let Some(url) = url {
		ensure!(!url.is_empty(), "product url is empty");
		ensure!(
			!url.chars().any(|c| c.is_whitespace() || c.is_control() || c == ')'),
			"product url {url:?} would not survive being written into a User-Agent comment"
		);
	}

	let comment = url.map(|url| format!(" (+{url})")).unwrap_or_default();
	let header = format!("{USER_AGENT} {name}/{version}{comment}");

	if USER_AGENT_WITH_PRODUCT.set(header).is_err() {
		log::debug!(
			"ignoring set_product({name:?}, {version:?}): the User-Agent already reads {:?}",
			user_agent()
		);
	}
	Ok(())
}

/// The `User-Agent` header actually sent: [`USER_AGENT`], plus the product
/// token if [`set_product`] has been called.
///
/// With no product set this is byte-identical to [`USER_AGENT`].
#[must_use]
pub fn user_agent() -> &'static str {
	USER_AGENT_WITH_PRODUCT.get().map_or(USER_AGENT, String::as_str)
}

/// Whether `text` is a non-empty RFC 9110 token, the only thing a product
/// name or version may be.
fn is_token(text: &str) -> bool {
	!text.is_empty()
		&& text
			.chars()
			.all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
}

mod data_reader;
mod data_reader_blob;
mod data_reader_file;
mod data_reader_http;
#[cfg(feature = "sftp")]
mod data_reader_sftp;
mod data_writer;
mod data_writer_blob;
mod data_writer_file;
#[cfg(feature = "sftp")]
mod data_writer_sftp;
mod network_reader;
#[cfg(feature = "sftp")]
mod network_writer;
pub(crate) mod retry;
#[cfg(feature = "sftp")]
mod sftp_blocking;
#[cfg(feature = "sftp")]
mod sftp_pool;
/// Host-key verification and SFTP session setup.
#[cfg(feature = "sftp")]
pub mod sftp_utils;
#[cfg(feature = "sftp")]
mod sftp_wrappers;
#[cfg(all(feature = "sftp", test))]
pub(crate) mod test_sftp_server;
#[cfg(feature = "sftp")]
pub use sftp_wrappers::*;
mod value_reader;
mod value_reader_blob;
mod value_reader_file;
mod value_reader_slice;
mod value_writer;
mod value_writer_blob;
mod value_writer_file;

pub use data_reader::*;
pub use data_reader_blob::*;
pub use data_reader_file::*;
pub use data_reader_http::*;
#[cfg(feature = "sftp")]
pub use data_reader_sftp::*;
pub use data_writer::*;
pub use data_writer_blob::*;
pub use data_writer_file::*;
#[cfg(feature = "sftp")]
pub use data_writer_sftp::*;
pub use value_reader::*;
pub use value_reader_blob::*;
pub use value_reader_file::*;
pub use value_reader_slice::*;
pub use value_writer::*;
pub use value_writer_blob::*;
pub use value_writer_file::*;

#[cfg(test)]
mod tests {
	use super::{USER_AGENT, is_token, set_product, user_agent};

	#[test]
	fn user_agent_has_expected_shape() {
		// e.g. "versatiles/4.1.5 (+https://versatiles.org)"
		assert!(USER_AGENT.starts_with("versatiles/"), "got: {USER_AGENT}");
		assert!(USER_AGENT.contains(env!("CARGO_PKG_VERSION")), "got: {USER_AGENT}");
		assert!(USER_AGENT.contains("(+https://versatiles.org)"), "got: {USER_AGENT}");
	}

	/// One test, not five, because the product is process-global: a second test
	/// setting it would race this one for which call is the first, and "the first
	/// call wins" is half of what there is to check. So the whole life cycle runs
	/// here, in order.
	#[test]
	fn product_token_is_appended_once() {
		// Nothing set yet: byte-identical to the constant, which is the property
		// every provider recognising VersaTiles traffic today depends on.
		assert_eq!(user_agent(), USER_AGENT);

		// A name that is not a token would emit a header parsing as two products.
		assert!(set_product("VersaTiles Studio", "0.4.0", None).is_err());
		assert!(set_product("VersaTiles-Studio", "0 4 0", None).is_err());
		// A `)` in the url would close the comment early; a space would end it.
		assert!(set_product("VersaTiles-Studio", "0.4.0", Some("https://x.org/a)b")).is_err());
		assert!(set_product("VersaTiles-Studio", "0.4.0", Some("")).is_err());
		assert_eq!(user_agent(), USER_AGENT, "a rejected call must not set anything");

		set_product("VersaTiles-Studio", "0.4.0", Some("https://versatiles.org/studio")).unwrap();
		assert_eq!(
			user_agent(),
			format!("{USER_AGENT} VersaTiles-Studio/0.4.0 (+https://versatiles.org/studio)")
		);

		// First call wins: a library that set it cannot be overridden mid-run.
		set_product("Something-Else", "9.9.9", None).unwrap();
		assert!(user_agent().ends_with("VersaTiles-Studio/0.4.0 (+https://versatiles.org/studio)"));
	}

	#[test]
	fn tokens_are_rfc9110_tokens() {
		assert!(is_token("VersaTiles-Studio"));
		assert!(is_token("0.4.0-beta.1"));
		assert!(is_token("a"));
		assert!(!is_token(""));
		assert!(!is_token("has space"));
		assert!(!is_token("has/slash"));
		assert!(!is_token("has(paren"));
	}
}

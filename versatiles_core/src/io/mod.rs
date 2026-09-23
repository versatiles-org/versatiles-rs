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

/// A URL written out for a person to read, with any password removed.
///
/// Error messages and log lines name the thing that failed, and for a remote
/// source that name is a URL the operator may have written a password into —
/// `sftp://user:hunter2@host/tiles.versatiles`. Interpolating it puts the
/// password on the terminal, into the CI log that scrolls past, and into
/// whatever collects that output.
///
/// The username is kept. It says which account was tried, which is what
/// somebody reading the failure needs, and it is not the secret. (SFTP's own
/// [`sftp_utils::display_name`] drops it as well, because it labels a
/// connection rather than explaining a failure.)
///
/// Not `Display`: a `Url` that reaches a person should say so at the call site,
/// so the ones that have not been thought about are visible.
#[must_use]
pub fn url_for_display(url: &reqwest::Url) -> String {
	if url.password().is_none() {
		return url.to_string();
	}

	let mut url = url.clone();
	// Both fail only for a URL that cannot have credentials in the first
	// place, which is then already safe to print.
	if url.set_password(None).is_err() {
		return url.to_string();
	}
	url.to_string()
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
mod sftp_pool;
/// Host-key verification and SFTP session setup.
#[cfg(feature = "sftp")]
pub mod sftp_utils;
#[cfg(feature = "sftp")]
mod sftp_wrappers;
/// In-process SFTP server for tests.
///
/// Behind the `test` feature as well as `cfg(test)` so the crates that depend on
/// this one can exercise their own SFTP paths against it — the alternative is
/// leaving their remote wiring untested, which is where the bugs are.
#[cfg(all(feature = "sftp", any(test, feature = "test")))]
pub mod test_sftp_server;
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

#[cfg(test)]
mod url_display_tests {
	use super::*;

	#[test]
	fn a_password_never_survives() {
		for input in [
			"sftp://alice:hunter2@example.org/tiles.versatiles",
			"https://alice:hunter2@example.org/tiles.json",
			"sftp://alice:@example.org/x",
		] {
			let url = reqwest::Url::parse(input).unwrap();
			let shown = url_for_display(&url);
			assert!(!shown.contains("hunter2"), "password leaked: {shown}");
			assert!(shown.contains("example.org"), "host should survive: {shown}");
		}
	}

	#[test]
	fn the_username_survives_because_it_says_which_account_was_tried() {
		let url = reqwest::Url::parse("sftp://alice:hunter2@example.org/x").unwrap();
		assert_eq!(url_for_display(&url), "sftp://alice@example.org/x");
	}

	#[test]
	fn a_url_without_credentials_is_unchanged() {
		for input in [
			"https://example.org/tiles.json",
			"sftp://example.org:2222/a/b.versatiles",
			"https://example.org/a?x=1#y",
		] {
			let url = reqwest::Url::parse(input).unwrap();
			assert_eq!(url_for_display(&url), input);
		}
	}
}

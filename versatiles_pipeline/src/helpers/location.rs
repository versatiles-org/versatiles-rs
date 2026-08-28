//! The two kinds of "where the data is" a VPL argument can name.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail, ensure};
use versatiles_container::DataLocation;

/// A path to a file on disk.
///
/// Thirteen arguments name one, and every one of them was a `String` — so a
/// consumer had no way to tell `filename` from `layer_name` except by reading
/// the doc comment, and offering a file picker meant matching on argument names
/// (#260).
///
/// # Why this refuses a URL, and why that changes nothing
///
/// Each of these arguments resolves its value and then calls
/// `DataLocation::to_path_buf`, which fails for a URL or a piped blob. So a URL
/// was already refused; it was refused after the pipeline had been built. The
/// type moves that refusal to where the value is decoded, which is where
/// `check` can see it.
///
/// The two arguments that genuinely take either — `from_container.filename` and
/// `filter.filename`, which open a tile container — are [`SourceLocation`]
/// instead.
///
/// # Why it does not parse through `DataLocation`
///
/// `DataLocation::parse` reads **all of stdin** when the input is `-`. That is
/// a deliberate feature of the CLI and a thing `check` must never do: an editor
/// asking what is wrong with a half-written pipeline would block on a pipe that
/// is never going to deliver. So the classification below is by inspection
/// only, and the resolving — which needs the VPL file's directory — stays in
/// the operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilePath(PathBuf);

impl FilePath {
	/// The path, unresolved: still relative to the VPL document if it was
	/// written that way.
	#[must_use]
	pub fn as_path(&self) -> &Path {
		&self.0
	}

	/// The location to resolve against the VPL document's directory.
	///
	/// Infallible, because the parser already decided this is a path.
	#[must_use]
	pub fn to_location(&self) -> DataLocation {
		DataLocation::Path(self.0.clone())
	}
}

impl std::fmt::Display for FilePath {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0.display())
	}
}

impl TryFrom<&str> for FilePath {
	type Error = anyhow::Error;

	fn try_from(text: &str) -> Result<Self> {
		ensure!(!text.trim().is_empty(), "a file path cannot be empty");

		match classify(text) {
			Location::Path => Ok(Self(PathBuf::from(text))),
			Location::Url => bail!("'{text}' is a URL, and this argument takes a path to a file"),
			Location::Stdin => bail!("'-' reads from standard input, which this argument cannot do"),
			Location::Inline => bail!("'{text}' is inline data, and this argument takes a path to a file"),
		}
	}
}

/// Where a tile container lives: a path, a URL, standard input, or inline data.
///
/// The distinction `from_container`'s doc comment carries in prose — "Path to
/// the container, or an `http`, `https` or `sftp` URL" — as something a
/// consumer can read. Everything a `DataLocation` accepts is accepted here, so
/// nothing is refused that used to build; what this adds is the name, and
/// its name — which is what a consumer reads to decide between a file picker
/// and a plain text box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLocation(String);

impl SourceLocation {
	/// The location to open, resolved by `DataLocation`'s own parser.
	///
	/// **Reads standard input** when the location is `-`, which is the point of
	/// writing `-` and the reason this is a method called at build time rather
	/// than something the `TryFrom<&str>` above does: `check` must not block on
	/// a pipe.
	///
	/// # Errors
	///
	/// Errors when reading standard input fails.
	pub fn to_location(&self) -> Result<DataLocation> {
		DataLocation::parse(&self.0)
	}
}

impl TryFrom<&str> for SourceLocation {
	type Error = anyhow::Error;

	fn try_from(text: &str) -> Result<Self> {
		ensure!(!text.trim().is_empty(), "a location cannot be empty");
		Ok(Self(text.to_string()))
	}
}

/// An absolute URL with a host.
///
/// One argument names one — `from_tilejson.url` — and it was a `String`, so a
/// typo like `htp://…` reached the HTTP client before anything objected. The
/// parser is `reqwest`'s, the same one that would have been asked later (#260).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpUrl(String);

impl HttpUrl {
	/// The URL as it was written.
	#[must_use]
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl TryFrom<&str> for HttpUrl {
	type Error = anyhow::Error;

	fn try_from(text: &str) -> Result<Self> {
		let url = reqwest::Url::parse(text.trim()).map_err(|e| anyhow::anyhow!("'{text}' is not a URL: {e}"))?;
		ensure!(url.has_host(), "'{text}' is a URL without a host");
		Ok(Self(text.trim().to_string()))
	}
}

impl std::fmt::Display for HttpUrl {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.0)
	}
}

/// What a location points at, as far as inspection can tell.
enum Location {
	Path,
	Url,
	Stdin,
	Inline,
}

/// Classifies a location the way `DataLocation::parse_with_stdin` does, minus
/// the reading — the same four branches, in the same order.
fn classify(text: &str) -> Location {
	let trimmed = text.trim();
	if text == "-" {
		Location::Stdin
	} else if trimmed.starts_with('(') && trimmed.ends_with(')') {
		Location::Inline
	} else if reqwest::Url::parse(text).is_ok_and(|url| url.has_host()) {
		Location::Url
	} else {
		Location::Path
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case::relative("data/cities.csv")]
	#[case::absolute("/srv/tiles/world.versatiles")]
	#[case::windows_ish("C:/tiles/world.mbtiles")]
	#[case::no_extension("cities")]
	fn a_path_is_a_path(#[case] text: &str) {
		assert_eq!(FilePath::try_from(text).unwrap().as_path(), Path::new(text));
	}

	/// Each of these already failed — after the pipeline was built, on
	/// `to_path_buf`. The type refuses them where `check` can see it.
	#[rstest]
	#[case::http("https://example.org/tiles.versatiles", "is a URL")]
	#[case::sftp("sftp://host/tiles.versatiles", "is a URL")]
	#[case::stdin("-", "standard input")]
	#[case::inline("(some inline data)", "is inline data")]
	#[case::empty("  ", "cannot be empty")]
	fn what_is_not_a_path_says_what_it_is(#[case] text: &str, #[case] expected: &str) {
		let error = FilePath::try_from(text).expect_err("should be rejected");
		assert!(error.to_string().contains(expected), "got: {error}");
	}

	/// A container may live anywhere, so this one refuses nothing that a
	/// `DataLocation` accepts — including `-`, which is a real way to pipe a
	/// container in. Nothing here reads that pipe: parsing is by inspection,
	/// and `to_location` is what the builder calls.
	#[rstest]
	#[case("world.versatiles")]
	#[case("https://example.org/tiles.versatiles")]
	#[case("sftp://host/tiles.versatiles")]
	#[case("-")]
	#[case("(inline)")]
	fn a_source_location_accepts_anywhere_data_can_live(#[case] text: &str) {
		assert!(SourceLocation::try_from(text).is_ok(), "{text} should be accepted");
	}

	#[rstest]
	#[case::http("https://example.org/tiles.json")]
	#[case::trailing_space(" https://example.org/tiles.json ")]
	fn a_url_is_a_url(#[case] text: &str) {
		assert_eq!(HttpUrl::try_from(text).unwrap().as_str(), text.trim());
	}

	#[rstest]
	// `htp:/…` parses — an unknown scheme is still a scheme — and fails on the
	// host, which is the more precise complaint of the two.
	#[case::scheme_typo("htp:/example.org", "without a host")]
	#[case::a_path("tiles/index.json", "is not a URL")]
	#[case::no_host("file:///tiles.json", "without a host")]
	fn what_is_not_a_url_says_so(#[case] text: &str, #[case] expected: &str) {
		let error = HttpUrl::try_from(text).expect_err("should be rejected");
		assert!(error.to_string().contains(expected), "got: {error}");
	}

	#[test]
	fn an_empty_location_is_refused_either_way() {
		assert!(SourceLocation::try_from("").is_err());
		assert!(FilePath::try_from("").is_err());
	}
}

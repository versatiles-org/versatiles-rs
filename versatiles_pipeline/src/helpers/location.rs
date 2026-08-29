//! The two kinds of "where the data is" a VPL argument can name.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail, ensure};
use versatiles_container::DataLocation;
use versatiles_core::TileFormat;

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

/// A path to a file `from_geo` can read, refused here if it cannot.
///
/// The extensions are the operation's own dispatch, moved to where the value is
/// decoded. Two things follow that a `FilePath` could not give:
/// [`Accepts::Only`](crate::vpl::Accepts::Only) in the argument metadata, so a
/// file dialog filters correctly instead of guessing from the node's name; and
/// the refusal reaching [`check`](crate::check_pipeline), because the
/// `VPLDecode` derive emits this `TryFrom` into
/// [`VPLFieldMeta::validate`](crate::vpl::VPLFieldMeta::validate). An editor
/// underlines `filename="notes.txt"` as it is typed rather than at build time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeoDataPath(FilePath);

impl GeoDataPath {
	/// Every extension `from_geo` reads, lowercase and without the dot.
	///
	/// The single source of truth: the operation dispatches on the value this
	/// type already accepted, and the metadata reports this list.
	pub const EXTENSIONS: &'static [&'static str] = &[
		"geojson",
		"json",
		"ndjson",
		"ndgeojson",
		"geojsonl",
		"geojsonseq",
		"shp",
	];

	/// The location to resolve against the VPL document's directory.
	#[must_use]
	pub fn to_location(&self) -> DataLocation {
		self.0.to_location()
	}

	/// The extension this path was accepted for, lowercase and without the dot.
	///
	/// Infallible: nothing else got past [`TryFrom`].
	#[must_use]
	pub fn extension(&self) -> String {
		self
			.0
			.as_path()
			.extension()
			.and_then(|s| s.to_str())
			.unwrap_or_default()
			.to_ascii_lowercase()
	}
}

impl TryFrom<&str> for GeoDataPath {
	type Error = anyhow::Error;

	fn try_from(text: &str) -> Result<Self> {
		let path = FilePath::try_from(text)?;
		let ext = path
			.as_path()
			.extension()
			.and_then(|s| s.to_str())
			.map(str::to_ascii_lowercase)
			.unwrap_or_default();
		ensure!(
			!ext.is_empty(),
			"file '{text}' has no extension; expected .{}",
			Self::EXTENSIONS.join(" / .")
		);
		ensure!(
			Self::EXTENSIONS.contains(&ext.as_str()),
			"unsupported file extension '.{ext}'; expected .{}",
			Self::EXTENSIONS.join(" / .")
		);
		Ok(Self(path))
	}
}

impl std::fmt::Display for GeoDataPath {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		self.0.fmt(f)
	}
}

/// A path to a single tile file, refused here if its extension names no format.
///
/// Delegates to [`TileFormat::try_from_str`], so a format added there is
/// accepted here and reported by the metadata without this list being touched —
/// the reason it is a function of `TileFormat` rather than a copy of its arms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileFilePath(FilePath);

impl TileFilePath {
	/// Every extension [`TileFormat`] names, lowercase and without the dot.
	///
	/// Kept beside `TileFormat::try_from_str` by
	/// `every_tile_extension_is_offered`, which fails if the two drift.
	pub const EXTENSIONS: &'static [&'static str] = &[
		"avif", "bin", "geojson", "jpeg", "jpg", "json", "mvt", "pbf", "png", "svg", "topojson", "webp",
	];

	/// The location to resolve against the VPL document's directory.
	#[must_use]
	pub fn to_location(&self) -> DataLocation {
		self.0.to_location()
	}
}

impl TryFrom<&str> for TileFilePath {
	type Error = anyhow::Error;

	fn try_from(text: &str) -> Result<Self> {
		let path = FilePath::try_from(text)?;
		TileFormat::try_from_path(path.as_path()).map_err(|_| {
			anyhow::anyhow!(
				"'{text}' does not name a tile format; expected .{}",
				Self::EXTENSIONS.join(" / .")
			)
		})?;
		Ok(Self(path))
	}
}

impl std::fmt::Display for TileFilePath {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		self.0.fmt(f)
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

	#[rstest]
	#[case("world.geojson")]
	#[case("world.GeoJSON")]
	#[case("data/places.geojsonl")]
	#[case("admin.shp")]
	fn a_geo_data_path_takes_what_from_geo_reads(#[case] text: &str) {
		assert!(GeoDataPath::try_from(text).is_ok(), "'{text}' should be accepted");
	}

	/// The refusal `from_geo` used to make at build time, made where the value
	/// is decoded — which is where `check` can see it.
	#[rstest]
	#[case::wrong_extension("notes.txt", "unsupported file extension")]
	#[case::no_extension("notes", "has no extension")]
	#[case::a_url("https://example.org/a.geojson", "is a URL")]
	fn a_geo_data_path_refuses_what_from_geo_cannot_read(#[case] text: &str, #[case] expected: &str) {
		let error = GeoDataPath::try_from(text).expect_err("should be rejected");
		assert!(error.to_string().contains(expected), "got: {error}");
	}

	/// The extension is lowercased on the way in, so the operation's dispatch
	/// sees one spelling.
	#[test]
	fn a_geo_data_path_reports_its_extension_lowercased() {
		assert_eq!(GeoDataPath::try_from("Places.GeoJSON").unwrap().extension(), "geojson");
	}

	#[rstest]
	#[case("tile.png")]
	#[case("tile.pbf")]
	#[case("tile.MVT")]
	fn a_tile_file_path_takes_what_tile_format_names(#[case] text: &str) {
		assert!(TileFilePath::try_from(text).is_ok(), "'{text}' should be accepted");
	}

	#[test]
	fn a_tile_file_path_refuses_what_names_no_format() {
		let error = TileFilePath::try_from("tile.txt").expect_err("should be rejected");
		assert!(
			error.to_string().contains("does not name a tile format"),
			"got: {error}"
		);
	}

	/// `EXTENSIONS` is published to consumers while `TileFormat` is what parses,
	/// so the two have to agree in both directions: everything offered parses,
	/// and nothing that parses is left off the list.
	#[test]
	fn every_tile_extension_is_offered() {
		for ext in TileFilePath::EXTENSIONS {
			assert!(
				TileFormat::try_from_str(ext).is_ok(),
				"'{ext}' is offered but names no format"
			);
		}
		for ext in [
			"avif", "bin", "geojson", "jpeg", "jpg", "json", "mvt", "pbf", "png", "svg", "topojson", "webp",
		] {
			assert!(
				TileFilePath::EXTENSIONS.contains(&ext),
				"'{ext}' parses as a tile format but is not offered"
			);
		}
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

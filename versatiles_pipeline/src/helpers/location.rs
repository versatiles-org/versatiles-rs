//! The kinds of "where the data is" a VPL argument can name.

use std::{
	path::{Path, PathBuf},
	sync::Arc,
};

use anyhow::{Context, Result, bail, ensure};
use versatiles_container::{DataLocation, open_url_reader};
use versatiles_core::{
	Blob, TileFormat,
	io::{location_for_display, url_for_display},
};

use crate::PipelineFactory;

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
/// `filter.filename`, which open a tile container — are [`ContainerLocation`]
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

	/// The path resolved against the VPL document's directory.
	///
	/// # Errors
	///
	/// Errors when the result is not a local path: a relative path in a VPL
	/// loaded from a URL resolves to a URL.
	pub fn resolve(&self, factory: &PipelineFactory) -> Result<PathBuf> {
		factory
			.resolve_location(&DataLocation::Path(self.0.clone()))?
			.to_path_buf()
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
			LocationKind::Path => Ok(Self(PathBuf::from(text))),
			LocationKind::Url => bail!("'{text}' is a URL, and this argument takes a path to a file"),
			LocationKind::Stdin => bail!("'-' reads from standard input, which this argument cannot do"),
			LocationKind::Inline => bail!("'{text}' is inline data, and this argument takes a path to a file"),
		}
	}
}

/// One file, on disk or behind a URL, that an operation reads whole.
///
/// `FilePath` with a URL let in: `http`, `https`, and `sftp` when the binary
/// has it — the schemes a container may come from, so "path or URL" means one
/// thing across the language. Standard input and inline data stay refused:
/// stdin may already be the container's, and inline text is not a file.
///
/// Like `FilePath`, parsing is by inspection only — `check` must not reach
/// the network any more than it may block on a pipe. Whether the URL answers
/// is found out when the pipeline is built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileLocation(String);

/// What [`FileLocation::open`] hands to a reader: a local file stays a path,
/// so it can be streamed, and a remote one arrives as bytes.
///
/// `Arc`, because a `Blob` is a `Vec<u8>` and a reader that needs its own
/// handle would otherwise copy the whole file.
#[derive(Clone, Debug)]
#[cfg_attr(
	not(test),
	expect(dead_code, reason = "no operation reads a FileInput yet; from_csv and from_geo will")
)]
pub enum FileInput {
	/// A local file, resolved against the VPL document's directory.
	Path(PathBuf),
	/// The content of a remote file.
	Bytes(Arc<Blob>),
}

#[cfg_attr(
	not(test),
	expect(
		dead_code,
		reason = "no operation takes a FileLocation yet; the next commits switch them over"
	)
)]
impl FileLocation {
	/// The location resolved against the VPL document's directory.
	fn resolve(&self, factory: &PipelineFactory) -> Result<DataLocation> {
		factory.resolve_location(&self.unresolved())
	}

	/// The location as written. A relative path stays relative.
	fn unresolved(&self) -> DataLocation {
		match classify(&self.0) {
			LocationKind::Url => match reqwest::Url::parse(&self.0) {
				Ok(url) => DataLocation::Url(url),
				Err(_) => unreachable!("'{}' was classified as a URL when it was parsed", self.0),
			},
			_ => DataLocation::Path(PathBuf::from(&self.0)),
		}
	}

	/// The file's whole content.
	///
	/// # Errors
	///
	/// Errors when the file cannot be read: a missing file, a URL that does
	/// not answer with success, or a body over `VERSATILES_MAX_HTTP_BODY_BYTES`.
	pub async fn read_all(&self, factory: &PipelineFactory) -> Result<Blob> {
		match self.resolve(factory)? {
			DataLocation::Path(path) => Blob::load_from_file(&path).with_context(|| format!("reading {}", path.display())),
			DataLocation::Url(url) => fetch(&url, factory).await,
			DataLocation::Blob(_) => unreachable!("a FileLocation never resolves to inline data"),
		}
	}

	/// The file's whole content as text.
	///
	/// # Errors
	///
	/// As [`read_all`](Self::read_all), and when the content is not UTF-8.
	pub async fn read_to_string(&self, factory: &PipelineFactory) -> Result<String> {
		self
			.read_all(factory)
			.await?
			.try_into_string()
			.with_context(|| format!("reading {self} as text"))
	}

	/// The file for a reader that streams: a local file as its path, a remote
	/// one fetched into memory.
	///
	/// # Errors
	///
	/// As [`read_all`](Self::read_all), for a URL. A local path is not
	/// opened here, so a missing file is the reader's to report.
	pub async fn open(&self, factory: &PipelineFactory) -> Result<FileInput> {
		match self.resolve(factory)? {
			DataLocation::Path(path) => Ok(FileInput::Path(path)),
			DataLocation::Url(url) => Ok(FileInput::Bytes(Arc::new(fetch(&url, factory).await?))),
			DataLocation::Blob(_) => unreachable!("a FileLocation never resolves to inline data"),
		}
	}

	/// The last segment of the path or of the URL's path, for defaults such as
	/// a layer name. `None` when there is none, as for `https://example.org/`.
	#[must_use]
	pub fn file_name(&self) -> Option<String> {
		self.unresolved().filename().ok().filter(|name| !name.is_empty())
	}
}

/// Fetches a remote file whole, saying so in the log: a large download has no
/// progress bar, and without a line before it the build looks stuck.
async fn fetch(url: &reqwest::Url, factory: &PipelineFactory) -> Result<Blob> {
	let display = url_for_display(url);
	log::info!("downloading {display}");
	let start = std::time::Instant::now();
	let reader = open_url_reader(url, None, &factory.runtime()).await?;
	let blob = reader.read_all().await?;
	log::info!("downloaded {display}: {} bytes in {:.1?}", blob.len(), start.elapsed());
	Ok(blob)
}

impl std::fmt::Display for FileLocation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&location_for_display(&self.0))
	}
}

impl TryFrom<&str> for FileLocation {
	type Error = anyhow::Error;

	fn try_from(text: &str) -> Result<Self> {
		ensure!(!text.trim().is_empty(), "a file location cannot be empty");

		match classify(text) {
			LocationKind::Path => Ok(Self(text.to_string())),
			LocationKind::Url => {
				let scheme = reqwest::Url::parse(text).map(|url| url.scheme().to_string())?;
				ensure!(
					matches!(scheme.as_str(), "http" | "https" | "sftp"),
					"'{}' is a {scheme} URL; a file can be read from an http, https or sftp URL",
					location_for_display(text)
				);
				Ok(Self(text.to_string()))
			}
			LocationKind::Stdin => bail!("'-' reads from standard input, which this argument cannot do"),
			LocationKind::Inline => bail!("'{text}' is inline data, and this argument takes a path or a URL"),
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

	/// The path resolved against the VPL document's directory.
	///
	/// # Errors
	///
	/// As [`FilePath::resolve`].
	pub fn resolve(&self, factory: &PipelineFactory) -> Result<PathBuf> {
		self.0.resolve(factory)
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

/// A single tile file, on disk or behind a URL, refused here if its extension
/// names no format.
///
/// The extension is that of the last path segment, so a URL's query string
/// does not get in the way (`tile.png?key=…` is a PNG). A URL without one
/// names no format and is refused, like a path without one.
///
/// Delegates to [`TileFormat::try_from_str`], so a format added there is
/// accepted here and reported by the metadata without this list being touched —
/// the reason it is a function of `TileFormat` rather than a copy of its arms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileFileLocation {
	location: FileLocation,
	format: TileFormat,
}

impl TileFileLocation {
	/// Every extension [`TileFormat`] names, lowercase and without the dot.
	///
	/// Kept beside `TileFormat::try_from_str` by
	/// `every_tile_extension_is_offered`, which fails if the two drift.
	pub const EXTENSIONS: &'static [&'static str] = &[
		"avif", "bin", "geojson", "jpeg", "jpg", "json", "mvt", "pbf", "png", "svg", "topojson", "webp",
	];

	/// The format the extension names.
	#[must_use]
	pub fn format(&self) -> TileFormat {
		self.format
	}

	/// The tile file's content.
	///
	/// # Errors
	///
	/// As [`FileLocation::read_all`].
	pub async fn read_all(&self, factory: &PipelineFactory) -> Result<Blob> {
		self.location.read_all(factory).await
	}
}

impl TryFrom<&str> for TileFileLocation {
	type Error = anyhow::Error;

	fn try_from(text: &str) -> Result<Self> {
		let location = FileLocation::try_from(text)?;
		let format = location
			.file_name()
			.and_then(|name| TileFormat::try_from_path(Path::new(&name)).ok())
			.ok_or_else(|| {
				anyhow::anyhow!(
					"'{location}' does not name a tile format; expected .{}",
					Self::EXTENSIONS.join(" / .")
				)
			})?;
		Ok(Self { location, format })
	}
}

impl std::fmt::Display for TileFileLocation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		self.location.fmt(f)
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
pub struct ContainerLocation(String);

impl ContainerLocation {
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

impl TryFrom<&str> for ContainerLocation {
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
enum LocationKind {
	Path,
	Url,
	Stdin,
	Inline,
}

/// Classifies a location the way `DataLocation::parse_with_stdin` does, minus
/// the reading — the same four branches, in the same order.
fn classify(text: &str) -> LocationKind {
	let trimmed = text.trim();
	if text == "-" {
		LocationKind::Stdin
	} else if trimmed.starts_with('(') && trimmed.ends_with(')') {
		LocationKind::Inline
	} else if reqwest::Url::parse(text).is_ok_and(|url| url.has_host()) {
		LocationKind::Url
	} else {
		LocationKind::Path
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
	#[case("tile.png", TileFormat::PNG)]
	#[case("tile.pbf", TileFormat::MVT)]
	#[case("tile.MVT", TileFormat::MVT)]
	#[case("https://example.org/tiles/empty.webp", TileFormat::WEBP)]
	#[case("https://example.org/tile.png?key=abc", TileFormat::PNG)]
	fn a_tile_file_location_takes_what_tile_format_names(#[case] text: &str, #[case] format: TileFormat) {
		assert_eq!(TileFileLocation::try_from(text).unwrap().format(), format);
	}

	#[rstest]
	#[case::unknown_extension("tile.txt")]
	#[case::no_extension("tile")]
	#[case::url_without_extension("https://example.org/tile?format=png")]
	fn a_tile_file_location_refuses_what_names_no_format(#[case] text: &str) {
		let error = TileFileLocation::try_from(text).expect_err("should be rejected");
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
		for ext in TileFileLocation::EXTENSIONS {
			assert!(
				TileFormat::try_from_str(ext).is_ok(),
				"'{ext}' is offered but names no format"
			);
		}
		for ext in [
			"avif", "bin", "geojson", "jpeg", "jpg", "json", "mvt", "pbf", "png", "svg", "topojson", "webp",
		] {
			assert!(
				TileFileLocation::EXTENSIONS.contains(&ext),
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
	fn a_container_location_accepts_anywhere_data_can_live(#[case] text: &str) {
		assert!(ContainerLocation::try_from(text).is_ok(), "{text} should be accepted");
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

	#[rstest]
	#[case::relative("data/style.json")]
	#[case::absolute("/srv/styles/style.json")]
	#[case::http("http://example.org/style.json")]
	#[case::https("https://example.org/style.json?key=abc")]
	#[case::sftp("sftp://user@host/styles/style.json")]
	fn a_file_location_is_a_path_or_a_readable_url(#[case] text: &str) {
		assert!(FileLocation::try_from(text).is_ok(), "{text} should be accepted");
	}

	#[rstest]
	#[case::ftp("ftp://user:secret@host/style.json", "is a ftp URL")]
	#[case::stdin("-", "standard input")]
	#[case::inline("(inline)", "inline data")]
	#[case::empty(" ", "cannot be empty")]
	fn a_file_location_refuses_what_it_cannot_read(#[case] text: &str, #[case] expected: &str) {
		let message = FileLocation::try_from(text).expect_err("should be refused").to_string();
		assert!(message.contains(expected), "got: {message}");
		assert!(!message.contains("secret"), "the password leaked: {message}");
	}

	#[test]
	fn a_file_location_is_shown_without_its_password() {
		let location = FileLocation::try_from("https://user:secret@example.org/style.json").unwrap();
		assert_eq!(location.to_string(), "https://user@example.org/style.json");
	}

	#[rstest]
	#[case::path("data/cities.csv", Some("cities.csv"))]
	#[case::url_with_query("https://example.org/a/cities.csv?key=abc", Some("cities.csv"))]
	#[case::url_without_path("https://example.org/", None)]
	fn a_file_location_names_its_file(#[case] text: &str, #[case] expected: Option<&str>) {
		let location = FileLocation::try_from(text).unwrap();
		assert_eq!(location.file_name().as_deref(), expected);
	}

	/// Serves `content` at `/file.json`.
	async fn serve(content: &str) -> String {
		crate::helpers::test_server::serve_files(vec![("/file.json", content.as_bytes().to_vec())]).await
	}

	fn factory_in(dir: &Path) -> PipelineFactory {
		PipelineFactory::new_runtime_reader(dir, versatiles_container::TilesRuntime::new_silent())
	}

	#[tokio::test]
	async fn a_local_file_is_read_relative_to_the_vpl() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("file.json"), "{}").unwrap();
		let factory = factory_in(dir.path());
		let location = FileLocation::try_from("file.json").unwrap();

		assert_eq!(location.read_all(&factory).await.unwrap().as_str(), "{}");
		match location.open(&factory).await.unwrap() {
			FileInput::Path(path) => assert_eq!(path, dir.path().join("file.json")),
			FileInput::Bytes(_) => panic!("a local file should stay a path, so it can be streamed"),
		}
	}

	#[tokio::test]
	async fn a_missing_local_file_names_its_path() {
		let dir = tempfile::tempdir().unwrap();
		let location = FileLocation::try_from("missing.json").unwrap();
		let error = location.read_all(&factory_in(dir.path())).await.unwrap_err();
		assert!(format!("{error:#}").contains("missing.json"), "got: {error:#}");
	}

	#[tokio::test]
	async fn a_url_is_fetched() {
		let base = serve(r#"{"version":8}"#).await;
		let dir = tempfile::tempdir().unwrap();
		let factory = factory_in(dir.path());
		let location = FileLocation::try_from(format!("{base}/file.json").as_str()).unwrap();

		assert_eq!(location.read_all(&factory).await.unwrap().as_str(), r#"{"version":8}"#);
		match location.open(&factory).await.unwrap() {
			FileInput::Bytes(blob) => assert_eq!(blob.as_str(), r#"{"version":8}"#),
			FileInput::Path(path) => panic!("a URL should arrive as bytes, got {path:?}"),
		}
	}

	#[tokio::test]
	async fn a_url_that_is_not_found_says_so_without_its_password() {
		let base = serve("{}").await.replace("http://", "http://user:secret@");
		let dir = tempfile::tempdir().unwrap();
		let location = FileLocation::try_from(format!("{base}/other.json").as_str()).unwrap();

		let message = format!("{:#}", location.read_all(&factory_in(dir.path())).await.unwrap_err());
		assert!(message.contains("404"), "got: {message}");
		assert!(!message.contains("secret"), "the password leaked: {message}");
	}

	#[test]
	fn an_empty_location_is_refused_either_way() {
		assert!(ContainerLocation::try_from("").is_err());
		assert!(FileLocation::try_from("").is_err());
		assert!(FilePath::try_from("").is_err());
	}
}

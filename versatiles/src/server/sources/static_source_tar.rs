use std::{
	collections::HashMap,
	env::current_dir,
	ffi::OsStr,
	fmt::Debug,
	path::{Component, Path},
	sync::Arc,
};

use anyhow::{Result, anyhow, bail, ensure};
use async_trait::async_trait;
use futures::StreamExt;
use tokio::io::AsyncReadExt;
use tokio_tar::{Archive, EntryType};
use versatiles_core::{
	Blob, TileCompression,
	compression::{TargetCompression, decompress_brotli, decompress_gzip, decompress_zstd},
	io::{DataReaderHttp, DataReaderTrait},
};
use versatiles_derive::context;

use super::{
	super::utils::{Url, guess_mime},
	SourceResponse,
	static_source::StaticSourceTrait,
};

/// The compression variants of one file. Blobs are shared, so links to a file
/// don't copy its content.
#[derive(Debug)]
struct FileEntry {
	mime: String,
	un: Option<Arc<Blob>>,
	gz: Option<Arc<Blob>>,
	br: Option<Arc<Blob>>,
	zstd: Option<Arc<Blob>>,
}

impl FileEntry {
	fn new(mime: String) -> Self {
		FileEntry {
			mime,
			un: None,
			gz: None,
			br: None,
			zstd: None,
		}
	}
}

pub struct TarFile {
	lookup: HashMap<String, FileEntry>,
	name: String,
}

impl TarFile {
	#[context("loading static tar file from path: {path:?}")]
	pub async fn from(path: &Path) -> Result<Self> {
		let path = current_dir()?.join(path).canonicalize()?;

		ensure!(path.exists(), "path {path:?} does not exist");
		ensure!(path.is_absolute(), "path {path:?} must be absolute");
		ensure!(path.is_file(), "path {path:?} must be a file");

		let bytes = tokio::fs::read(&path).await?;

		let filename = path
			.file_name()
			.and_then(|s| s.to_str())
			.ok_or_else(|| anyhow!("path {path:?} has no valid UTF-8 filename"))?;
		let name = path.to_str().expect("path is valid UTF-8 (checked above)").to_owned();

		Self::from_bytes(Blob::from(bytes), filename, name).await
	}

	#[context("loading static tar file from URL: {url}")]
	pub async fn from_url(url: &reqwest::Url) -> Result<Self> {
		let reader = DataReaderHttp::try_from(url)?;
		let data = reader.read_all().await?;
		let filename = url
			.path_segments()
			.and_then(|mut s| s.next_back())
			.unwrap_or("remote.tar");
		Self::from_bytes(data, filename, url.to_string()).await
	}

	async fn from_bytes(mut data: Blob, filename: &str, name: String) -> Result<Self> {
		for part in filename.rsplit('.') {
			match part {
				"tar" => break,
				"gz" => data = decompress_gzip(&data)?,
				"br" => data = decompress_brotli(&data)?,
				"zst" => data = decompress_zstd(&data)?,
				_ => bail!("{filename:?} must be a name of a tar file"),
			}
		}

		let mut archive = Archive::new(data.as_slice());

		// Links may point at entries that come later in the archive, or at other
		// links, so every entry is collected first and links are resolved after.
		let mut nodes: HashMap<String, Node> = HashMap::new();
		let mut order: Vec<String> = Vec::new();
		let mut entries = archive.entries()?;
		while let Some(file_result) = entries.next().await {
			let Ok(mut file) = file_result else {
				continue;
			};

			let entry_type = file.header().entry_type();
			if !matches!(entry_type, EntryType::Regular | EntryType::Link | EntryType::Symlink) {
				continue;
			}

			let entry_path = file.path()?;
			let Some(entry_name) = resolve_path("", &entry_path) else {
				log::warn!("skipping tar entry with an invalid name: {entry_path:?}");
				continue;
			};
			drop(entry_path);

			let node = if entry_type == EntryType::Regular {
				let mut buffer = Vec::new();
				file.read_to_end(&mut buffer).await?;
				Node::File(Arc::new(Blob::from(buffer)))
			} else {
				let Ok(Some(link_name)) = file.link_name() else {
					log::warn!("skipping tar link {entry_name:?} without a readable target");
					continue;
				};
				// Hardlink targets are archive paths, symlink targets are relative
				// to the directory of the link. An absolute symlink points outside
				// the archive.
				let target = if entry_type == EntryType::Link {
					resolve_path("", &link_name)
				} else if link_name.has_root() {
					None
				} else {
					resolve_path(parent_dir(&entry_name), &link_name)
				};
				let Some(target) = target else {
					log::warn!("skipping tar link {entry_name:?}: target {link_name:?} is outside the archive");
					continue;
				};
				Node::Link(target)
			};

			if nodes.insert(entry_name.clone(), node).is_none() {
				order.push(entry_name);
			}
		}

		let mut lookup: HashMap<String, FileEntry> = HashMap::new();
		for entry_name in &order {
			let Some(blob) = resolve_node(&nodes, entry_name) else {
				log::warn!("skipping tar link {entry_name:?}: target is missing or more than {MAX_LINK_DEPTH} links away");
				continue;
			};
			add_file(&mut lookup, entry_name, blob);
		}

		Ok(Self { lookup, name })
	}
}

/// A tar entry before links are resolved.
enum Node {
	File(Arc<Blob>),
	/// Normalised archive path of the link target.
	Link(String),
}

/// Maximum number of links followed to reach a file, so link loops terminate.
const MAX_LINK_DEPTH: usize = 8;

/// Follows links from `name` until a regular file is reached.
fn resolve_node<'a>(nodes: &'a HashMap<String, Node>, name: &str) -> Option<&'a Arc<Blob>> {
	let mut node = nodes.get(name)?;
	for _ in 0..=MAX_LINK_DEPTH {
		match node {
			Node::File(blob) => return Some(blob),
			Node::Link(target) => node = nodes.get(target)?,
		}
	}
	None
}

/// Joins `path` onto the archive directory `base` and normalises it to `a/b/c`.
///
/// Leading `/` and `.` components are dropped, `..` removes the previous component.
/// Returns `None` if the path is not UTF-8 or escapes the archive root.
fn resolve_path(base: &str, path: &Path) -> Option<String> {
	let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
	for component in path.components() {
		match component {
			Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
			Component::ParentDir => {
				parts.pop()?;
			}
			Component::Normal(part) => parts.push(part.to_str()?),
		}
	}
	Some(parts.join("/"))
}

/// Returns the directory part of a normalised archive path.
fn parent_dir(name: &str) -> &str {
	name.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Registers a file under its path, as a compression variant if it ends in `.br`, `.gz` or
/// `.zst`. An `index.html` is registered for its directory, too.
fn add_file(lookup: &mut HashMap<String, FileEntry>, entry_name: &str, blob: &Arc<Blob>) {
	use TileCompression::{Brotli, Gzip, Uncompressed, Zstd};

	let mut path = entry_name.to_owned();
	let compression = TileCompression::from_filename(&mut path);
	let path = path.as_str();

	let Some(filename) = Path::new(path).file_name() else {
		return;
	};
	let mime = guess_mime(Path::new(filename));

	let mut add = |name: &str| {
		log::trace!("Adding file from tar: {name} ({compression:?})");

		let versions = lookup
			.entry(name.to_owned())
			.or_insert_with(|| FileEntry::new(mime.clone()));
		let blob = Some(Arc::clone(blob));
		match compression {
			Uncompressed => versions.un = blob,
			Gzip => versions.gz = blob,
			Brotli => versions.br = blob,
			Zstd => versions.zstd = blob,
		}
	};

	if filename == OsStr::new("index.html") {
		add(parent_dir(path));
	}
	add(path);
}

#[async_trait]
impl StaticSourceTrait for TarFile {
	#[cfg(test)]
	fn type_name(&self) -> &str {
		"tar"
	}

	#[cfg(test)]
	fn name(&self) -> &str {
		&self.name
	}

	async fn get_data(&self, url: &Url, accept: &TargetCompression) -> Option<SourceResponse> {
		use TileCompression::{Brotli, Gzip, Uncompressed, Zstd};

		let file_entry = self.lookup.get(&url.str[1..])?;

		if accept.contains(Brotli)
			&& let Some(blob) = &file_entry.br
		{
			return SourceResponse::new_some(Blob::clone(blob), Brotli, &file_entry.mime);
		}

		if accept.contains(Zstd)
			&& let Some(blob) = &file_entry.zstd
		{
			return SourceResponse::new_some(Blob::clone(blob), Zstd, &file_entry.mime);
		}

		if accept.contains(Gzip)
			&& let Some(blob) = &file_entry.gz
		{
			return SourceResponse::new_some(Blob::clone(blob), Gzip, &file_entry.mime);
		}

		if let Some(blob) = &file_entry.un {
			return SourceResponse::new_some(Blob::clone(blob), Uncompressed, &file_entry.mime);
		}

		if let Some(blob) = &file_entry.br {
			return SourceResponse::new_some(Blob::clone(blob), Brotli, &file_entry.mime);
		}

		if let Some(blob) = &file_entry.zstd {
			return SourceResponse::new_some(Blob::clone(blob), Zstd, &file_entry.mime);
		}

		if let Some(blob) = &file_entry.gz {
			return SourceResponse::new_some(Blob::clone(blob), Gzip, &file_entry.mime);
		}

		None
	}
}

impl Debug for TarFile {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TarFile").field("name", &self.name).finish()
	}
}

#[cfg(test)]
mod tests {

	use assert_fs::NamedTempFile;
	use rstest::rstest;
	use versatiles_container::{
		MockReader, MockReaderProfile, TileSource, TilesConverterParameters, TilesRuntime, convert_tiles_container,
	};
	use versatiles_core::TilePyramid;

	use super::*;

	pub async fn make_test_tar(compression: TileCompression) -> NamedTempFile {
		// get dummy reader
		let reader = MockReader::new_mock_profile(MockReaderProfile::Pbf)
			.unwrap()
			.into_shared();

		// get to test container converter
		let container_file = NamedTempFile::new("temp.tar").unwrap();

		let parameters = TilesConverterParameters {
			tile_compression: Some(compression),
			tile_pyramid: Some(TilePyramid::new_full_up_to(0)),
			..TilesConverterParameters::default()
		};
		let runtime = TilesRuntime::default();

		convert_tiles_container(reader, parameters, &container_file, runtime)
			.await
			.unwrap();

		container_file
	}

	#[tokio::test]
	async fn small_stuff() {
		let file = make_test_tar(TileCompression::Uncompressed).await;

		let tar_file = TarFile::from(&file).await.unwrap();

		assert!(tar_file.name().ends_with("temp.tar"));
		assert!(format!("{tar_file:?}").starts_with("TarFile { name:"));
	}

	#[tokio::test]
	async fn from_non_existing_path() {
		let path = Path::new("path/to/non-existing/file.tar");
		assert!(TarFile::from(path).await.is_err());
	}

	#[tokio::test]
	async fn from_directory() {
		let path = Path::new(".");
		assert!(TarFile::from(path).await.is_err());
	}

	#[rstest]
	#[case(TileCompression::Uncompressed)]
	#[case(TileCompression::Gzip)]
	#[case(TileCompression::Brotli)]
	#[case(TileCompression::Zstd)]
	#[tokio::test]
	async fn test_get_data(#[case] compression_tar: TileCompression) -> Result<()> {
		let file = make_test_tar(compression_tar).await;
		let tar_file = TarFile::from(&file).await?;

		for compression_accept in [
			TileCompression::Uncompressed,
			TileCompression::Gzip,
			TileCompression::Brotli,
			TileCompression::Zstd,
		] {
			let accept = TargetCompression::from(compression_accept);

			let result = tar_file.get_data(&Url::from("non_existing_file"), &accept).await;
			assert!(result.is_none());

			let result = tar_file.get_data(&Url::from("tiles.json"), &accept).await;
			assert!(result.is_some());

			let result = result.unwrap();

			if result.compression == TileCompression::Uncompressed {
				assert_eq!(
					result.blob.as_str(),
					"{\"tile_format\":\"application/vnd.mapbox-vector-tile\",\"tile_schema\":\"other\",\"tile_type\":\"vector\",\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
				);
			}

			assert_eq!(result.mime, "application/json");
			assert_eq!(result.compression, compression_tar);
		}

		Ok(())
	}

	/// Builds an uncompressed tar from `(path, kind)` entries, where `kind` is
	/// `File(content)`, `Hard(target)` or `Sym(target)`.
	async fn make_link_tar(entries: &[(&str, TestEntry<'_>)]) -> Result<TarFile> {
		let mut builder = tokio_tar::Builder::new(Vec::new());
		for (path, kind) in entries {
			let mut header = tokio_tar::Header::new_gnu();
			header.set_mode(0o644);
			let content: &[u8] = match kind {
				TestEntry::File(content) => content.as_bytes(),
				TestEntry::Hard(target) => {
					header.set_entry_type(EntryType::Link);
					header.set_link_name(target)?;
					&[]
				}
				TestEntry::Sym(target) => {
					header.set_entry_type(EntryType::Symlink);
					header.set_link_name(target)?;
					&[]
				}
			};
			header.set_size(content.len() as u64);
			builder.append_data(&mut header, path, content).await?;
		}
		let bytes = builder.into_inner().await?;
		TarFile::from_bytes(Blob::from(bytes), "links.tar", "links.tar".to_owned()).await
	}

	enum TestEntry<'a> {
		File(&'a str),
		Hard(&'a str),
		Sym(&'a str),
	}

	#[tokio::test]
	async fn serves_links() -> Result<()> {
		use TestEntry::{File, Hard, Sym};
		use TileCompression::{Brotli, Uncompressed};

		let tar_file = make_link_tar(&[
			("early.txt", Hard("dir/file.txt")),
			("dir/file.txt", File("content")),
			("dir/file.txt.br", File("brotli content")),
			("other/hard.txt", Hard("./dir/file.txt")),
			("other/hard.txt.br", Hard("dir/file.txt.br")),
			("other/sym.txt", Sym("../dir/file.txt")),
			("chain.txt", Sym("other/sym.txt")),
			("site/index.html", Sym("../dir/file.txt")),
			("dangling.txt", Hard("missing.txt")),
			("dir/escape.txt", Sym("../../file.txt")),
			("absolute.txt", Sym("/dir/file.txt")),
			("loop_a.txt", Sym("loop_b.txt")),
			("loop_b.txt", Sym("loop_a.txt")),
		])
		.await?;

		let get = async |path: &str, accept: TileCompression| {
			tar_file
				.get_data(&Url::from(path), &TargetCompression::from(accept))
				.await
				.map(|r| (r.blob.as_str().to_owned(), r.compression, r.mime))
		};

		let plain = Some((
			"content".to_owned(),
			Uncompressed,
			"text/plain; charset=utf-8".to_owned(),
		));
		for path in [
			"dir/file.txt",
			"early.txt",
			"other/hard.txt",
			"other/sym.txt",
			"chain.txt",
		] {
			assert_eq!(get(path, Uncompressed).await, plain, "{path}");
		}

		let brotli = Some((
			"brotli content".to_owned(),
			Brotli,
			"text/plain; charset=utf-8".to_owned(),
		));
		assert_eq!(get("dir/file.txt", Brotli).await, brotli);
		assert_eq!(get("other/hard.txt", Brotli).await, brotli);
		// The symlink only links the uncompressed variant.
		assert_eq!(get("other/sym.txt", Brotli).await, plain);

		let html = Some((
			"content".to_owned(),
			Uncompressed,
			"text/html; charset=utf-8".to_owned(),
		));
		assert_eq!(get("site", Uncompressed).await, html);
		assert_eq!(get("site/index.html", Uncompressed).await, html);

		for path in [
			"dangling.txt",
			"dir/escape.txt",
			"absolute.txt",
			"loop_a.txt",
			"loop_b.txt",
		] {
			assert_eq!(get(path, Uncompressed).await, None, "{path}");
		}

		// Links share the buffer of their target.
		let target = tar_file.lookup["dir/file.txt"].un.as_ref().unwrap();
		assert!(Arc::ptr_eq(
			target,
			tar_file.lookup["other/hard.txt"].un.as_ref().unwrap()
		));
		assert!(Arc::ptr_eq(target, tar_file.lookup["chain.txt"].un.as_ref().unwrap()));

		Ok(())
	}

	#[test]
	fn resolve_path_normalises() {
		let resolve = |base, path| resolve_path(base, Path::new(path));
		assert_eq!(resolve("", "./a/b.txt").as_deref(), Some("a/b.txt"));
		assert_eq!(resolve("", "/a/./b.txt").as_deref(), Some("a/b.txt"));
		assert_eq!(resolve("", ".hidden/b.txt").as_deref(), Some(".hidden/b.txt"));
		assert_eq!(resolve("a/b", "../c.txt").as_deref(), Some("a/c.txt"));
		assert_eq!(resolve("a/b", "../../c.txt").as_deref(), Some("c.txt"));
		assert_eq!(resolve("a/b", "../../../c.txt"), None);
		assert_eq!(resolve("", "a/../../c.txt"), None);
	}
}

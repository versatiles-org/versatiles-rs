use std::{
	env::current_dir,
	fmt::Debug,
	fs::File,
	io::{BufReader, Read},
	path::{Path, PathBuf},
};

use anyhow::{Result, anyhow, ensure};
use async_trait::async_trait;
use versatiles_core::{Blob, TileCompression, compression::TargetCompression};
use versatiles_derive::context;

use super::{
	super::{Url, utils::guess_mime},
	SourceResponse,
	static_source::StaticSourceTrait,
};

// Folder struct definition
#[derive(Clone)]
#[expect(
	clippy::struct_field_names,
	reason = "`self.folder` is the served root; the third field only just crossed clippy's threshold"
)]
pub struct Folder {
	folder: PathBuf,
	name: String,
	/// Whether a symlink may resolve to a file outside `folder`.
	///
	/// Symlinks *within* the folder work either way — their target is already
	/// being served. This is only about one pointing out of it.
	follow_symlinks: bool,
}

impl Folder {
	// Constructor for the Folder struct
	#[context("loading static folder from path: {path:?}")]
	pub fn from(path: &Path, follow_symlinks: bool) -> Result<Folder> {
		let mut folder = current_dir()?;
		folder.push(Path::new(path));
		folder = folder.canonicalize()?;

		// Check that the folder exists, is absolute and is a directory
		ensure!(folder.exists(), "path {folder:?} does not exist");
		ensure!(folder.is_absolute(), "path {folder:?} must be absolute");
		ensure!(folder.is_dir(), "path {folder:?} must be a directory");

		folder = folder.canonicalize()?;

		// Create a new Folder struct with the given path and name
		Ok(Folder {
			folder,
			name: path
				.to_str()
				.ok_or_else(|| anyhow!("path {path:?} is not valid UTF-8"))?
				.to_owned(),
			follow_symlinks,
		})
	}
}

#[async_trait]
impl StaticSourceTrait for Folder {
	#[cfg(test)]
	fn type_name(&self) -> &str {
		"folder"
	}

	// Returns the name of the folder
	#[cfg(test)]
	fn name(&self) -> &str {
		&self.name
	}

	// Gets the data at the given path and responds with a compressed or uncompressed version
	// based on the accept header
	async fn get_data(&self, url: &Url, _accept: &TargetCompression) -> Option<SourceResponse> {
		let mut local_path = url.to_pathbug(&self.folder);

		// If the path is a directory, append 'index.html'
		if local_path.is_dir() {
			local_path.push("index.html");
		}

		// If the local path is not a subpath of the folder, return not found.
		// Lexical only — `resolve` below is what accounts for symlinks.
		if !local_path.starts_with(&self.folder) {
			return None;
		}

		let mime = guess_mime(&local_path);

		// The plain file, then the precompressed sidecars (".br", ".gz"). Each
		// candidate is resolved separately: a `.br` sidecar can be a symlink
		// even when the file beside it is not.
		let (file, compression) = [
			("", TileCompression::Uncompressed),
			(".br", TileCompression::Brotli),
			(".gz", TileCompression::Gzip),
		]
		.into_iter()
		.find_map(|(suffix, compression)| {
			let candidate = if suffix.is_empty() {
				local_path.clone()
			} else {
				PathBuf::from(format!("{}{suffix}", local_path.display()))
			};
			let resolved = self.resolve(&candidate)?;
			File::open(resolved).ok().map(|file| (file, compression))
		})?;

		let mut buffer = Vec::new();
		BufReader::new(file).read_to_end(&mut buffer).ok()?;

		SourceResponse::new_some(Blob::from(buffer), compression, &mime)
	}
}

impl Folder {
	/// The real path `candidate` names, or `None` if it may not be served.
	///
	/// `to_pathbug` already refuses `..` as a path component, but that is a
	/// check on the *text* of the request. A symlink inside the folder is not
	/// text: `ln -s /etc/passwd public/passwd` makes `GET /passwd` a request for
	/// a file the operator never put there, and `File::open` follows it without
	/// comment. The lexical check upstream cannot see that, because the path it
	/// examines really is under the folder — it is the filesystem that leaves.
	///
	/// So the path is canonicalised, which resolves every link in it, and the
	/// result checked against the (already canonical) root. Links that stay
	/// inside are unaffected: their target is being served anyway.
	///
	/// `--follow-symlinks` turns the check off for operators who deliberately
	/// serve a tree of links — nginx follows them by default, so this is a
	/// posture choice rather than a fault to be fixed. Off by default, because a
	/// tool pointed at an arbitrary directory should not hand out whatever that
	/// directory happens to reference.
	fn resolve(&self, candidate: &Path) -> Option<PathBuf> {
		// Also fails for a path that does not exist, which is the 404 path and
		// wants no further comment.
		let real = candidate.canonicalize().ok()?;

		if self.follow_symlinks || real.starts_with(&self.folder) {
			return Some(real);
		}

		log::warn!(
			"refusing to serve {candidate:?}: it resolves to {real:?}, outside the served folder {:?}. \
			 Pass --follow-symlinks if that is deliberate",
			self.folder
		);
		None
	}
}

impl Debug for Folder {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Folder")
			.field("folder", &self.folder)
			.field("name", &self.name)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test() {
		// Create a new Folder instance
		let folder = Folder::from(Path::new("../testdata"), false).unwrap();

		let debug: String = format!("{folder:?}");
		assert!(debug.starts_with("Folder { folder: \""));
		assert!(debug.ends_with("testdata\", name: \"../testdata\" }"));

		// Test get_name function
		assert_eq!(folder.name(), "../testdata");

		// Test get_data function with a non-existent file
		let result = folder
			.get_data(&Url::from("recipes/Queijo.txt"), &TargetCompression::from_none())
			.await;
		assert!(result.is_none());

		// Test get_data function with an existing uncompressed file
		let result = folder
			.get_data(&Url::from("berlin.mbtiles"), &TargetCompression::from_none())
			.await;
		assert!(result.is_some());

		let result = result.unwrap();
		assert_eq!(result.blob.len(), 11481088);
		assert_eq!(result.compression, TileCompression::Uncompressed);
	}

	#[tokio::test]
	async fn directory_with_index_html() {
		// Setup: Create a temporary directory and place an index.html file inside it
		let temp_dir = assert_fs::TempDir::new().unwrap();
		let dir_path = temp_dir.path().join("testdir");
		std::fs::create_dir(&dir_path).unwrap_or_default();

		let index_path = dir_path.join("index.html");
		std::fs::write(&index_path, b"Hello, world!").unwrap();

		// Test initialization with the temporary directory
		let folder = Folder::from(temp_dir.path(), false).unwrap();

		// Attempt to retrieve data from the directory, expecting to get the contents of index.html
		let response = folder
			.get_data(&Url::from("testdir"), &TargetCompression::from_none())
			.await
			.unwrap();

		let result = response.blob.as_str();
		assert_eq!(result, "Hello, world!");
		assert_eq!(response.compression, TileCompression::Uncompressed);
	}

	/// A request path that climbs out of the served directory must not reach the
	/// file it points at. Before `Url` resolved dot segments, `GET /../secret.txt`
	/// returned the file's contents with status 200: `Path::starts_with` compares
	/// components lexically, so `<folder>/../secret.txt` passed the containment
	/// check and the operating system resolved the `..` on open.
	#[tokio::test]
	async fn path_traversal_is_rejected() {
		let temp_dir = assert_fs::TempDir::new().unwrap();
		let public = temp_dir.path().join("public");
		std::fs::create_dir(&public).unwrap();
		std::fs::write(public.join("index.html"), b"public").unwrap();
		std::fs::write(temp_dir.path().join("secret.txt"), b"TOP-SECRET").unwrap();

		let folder = Folder::from(&public, false).unwrap();

		// The file inside the folder is still served.
		assert!(
			folder
				.get_data(&Url::from("index.html"), &TargetCompression::from_none())
				.await
				.is_some()
		);

		for request in [
			"/../secret.txt",
			"/../../secret.txt",
			"/./../secret.txt",
			"//../secret.txt",
		] {
			assert!(
				folder
					.get_data(&Url::from(request), &TargetCompression::from_none())
					.await
					.is_none(),
				"{request} escaped the served folder"
			);
		}
	}

	#[tokio::test]
	async fn test_compressed_files() {
		// Setup: Create a temporary directory with Brotli and Gzip compressed files
		let temp_dir = assert_fs::TempDir::new().unwrap();
		let file_path = temp_dir.path().join("compressed.txt");

		// Create Brotli-compressed file
		let br_file_path = file_path.with_extension("txt.br");
		std::fs::write(&br_file_path, b"Brotli compressed content").unwrap();

		// Create Gzip-compressed file
		let gz_file_path = file_path.with_extension("txt.gz");
		std::fs::write(&gz_file_path, b"Gzip compressed content").unwrap();

		// Initialize folder and test get_data with Brotli file
		let folder = Folder::from(temp_dir.path(), false).unwrap();

		// Test Brotli compression
		let response_br = folder
			.get_data(&Url::from("compressed.txt"), &TargetCompression::from_none())
			.await
			.unwrap();

		assert_eq!(response_br.blob.as_str(), "Brotli compressed content");
		assert_eq!(response_br.compression, TileCompression::Brotli);

		// Remove Brotli file to test Gzip fallback
		std::fs::remove_file(&br_file_path).unwrap();

		// Test Gzip compression
		let response_gz = folder
			.get_data(&Url::from("compressed.txt"), &TargetCompression::from_none())
			.await
			.unwrap();

		assert_eq!(response_gz.blob.as_str(), "Gzip compressed content");
		assert_eq!(response_gz.compression, TileCompression::Gzip);

		// Cleanup
		temp_dir.close().unwrap();
	}
}

#[cfg(test)]
mod symlink_tests {
	use super::*;
	use crate::server::sources::static_source::StaticSourceTrait;

	/// A tree with a secret outside the served folder, a symlink pointing at
	/// it, and a symlink that stays inside.
	fn tree() -> (assert_fs::TempDir, PathBuf) {
		let temp = assert_fs::TempDir::new().unwrap();
		let public = temp.path().join("public");
		std::fs::create_dir(&public).unwrap();
		std::fs::write(public.join("index.html"), b"public").unwrap();
		std::fs::write(temp.path().join("secret.txt"), b"TOP-SECRET").unwrap();

		#[cfg(unix)]
		{
			std::os::unix::fs::symlink(temp.path().join("secret.txt"), public.join("leak.txt")).unwrap();
			std::os::unix::fs::symlink(public.join("index.html"), public.join("inside.html")).unwrap();
		}

		(temp, public)
	}

	/// The lexical check upstream cannot see this: the requested path really is
	/// under the served folder, and it is the filesystem that leaves.
	#[cfg(unix)]
	#[tokio::test]
	async fn a_symlink_out_of_the_folder_is_refused_by_default() {
		let (_temp, public) = tree();
		let folder = Folder::from(&public, false).unwrap();

		assert!(
			folder
				.get_data(&Url::from("leak.txt"), &TargetCompression::from_none())
				.await
				.is_none(),
			"a symlink pointing outside the folder was served"
		);
	}

	/// Off by default does not mean "no symlinks": one whose target is inside
	/// the folder is serving a file that is being served anyway.
	#[cfg(unix)]
	#[tokio::test]
	async fn a_symlink_inside_the_folder_still_works() {
		let (_temp, public) = tree();
		let folder = Folder::from(&public, false).unwrap();

		assert!(
			folder
				.get_data(&Url::from("inside.html"), &TargetCompression::from_none())
				.await
				.is_some(),
			"a symlink staying inside the folder was refused"
		);
	}

	/// The flag is what an operator serving a tree of links deliberately turns
	/// on.
	#[cfg(unix)]
	#[tokio::test]
	async fn the_flag_allows_a_symlink_to_leave() {
		let (_temp, public) = tree();
		let folder = Folder::from(&public, true).unwrap();

		let response = folder
			.get_data(&Url::from("leak.txt"), &TargetCompression::from_none())
			.await
			.expect("--follow-symlinks should serve it");
		assert_eq!(response.blob.as_slice(), b"TOP-SECRET");
	}

	/// An ordinary file is unaffected either way.
	#[cfg(unix)]
	#[tokio::test]
	async fn ordinary_files_are_unaffected() {
		let (_temp, public) = tree();
		for follow in [false, true] {
			let folder = Folder::from(&public, follow).unwrap();
			assert!(
				folder
					.get_data(&Url::from("index.html"), &TargetCompression::from_none())
					.await
					.is_some(),
				"follow_symlinks={follow}"
			);
		}
	}
}

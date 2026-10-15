//! Where a feature source reads its bytes from.

use std::{
	fs::File,
	io::{Cursor, Read},
	path::{Path, PathBuf},
	sync::Arc,
};

use anyhow::{Context, Result};
use versatiles_core::Blob;

/// The bytes a source reads: a local file, streamed from disk, or a file that
/// is already in memory — fetched from a URL, say.
///
/// A source never needs both, and reading from disk must stay streaming for a
/// multi-GB input, so this keeps a local file a path rather than loading it.
#[derive(Clone)]
pub enum SourceInput {
	/// A local file.
	Path(PathBuf),
	/// A file's whole content. `label` names it in messages — a URL, without
	/// its password — and its last segment gives the default layer name.
	///
	/// `Arc`, because a `Blob` is a `Vec<u8>`, and a source that is cloned
	/// would otherwise copy the whole file.
	Bytes {
		/// What the content is called in messages.
		label: String,
		/// The content.
		bytes: Arc<Blob>,
	},
}

impl SourceInput {
	/// Opens the input for reading from the start.
	///
	/// # Errors
	///
	/// Errors when a local file cannot be opened. Bytes always open.
	pub fn open(&self) -> Result<Box<dyn Read + Send>> {
		Ok(match self {
			SourceInput::Path(path) => Box::new(File::open(path).with_context(|| format!("opening {}", path.display()))?),
			SourceInput::Bytes { bytes, .. } => Box::new(Cursor::new(SharedBytes(Arc::clone(bytes)))),
		})
	}

	/// What the input is called in messages: the path, or the label.
	#[must_use]
	pub fn label(&self) -> String {
		match self {
			SourceInput::Path(path) => path.display().to_string(),
			SourceInput::Bytes { label, .. } => label.clone(),
		}
	}

	/// The size in bytes, for a progress bar; `0` for a file that cannot be
	/// read, which leaves the reporting to the open.
	#[must_use]
	pub fn len(&self) -> u64 {
		match self {
			SourceInput::Path(path) => std::fs::metadata(path).map_or(0, |m| m.len()),
			SourceInput::Bytes { bytes, .. } => bytes.len(),
		}
	}

	/// Whether there is nothing to read.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// The file name without its extension: a path's stem, or that of the
	/// label's last segment with any query or fragment cut off.
	#[must_use]
	pub fn file_stem(&self) -> Option<String> {
		self.file_name_path()?.file_stem()?.to_str().map(str::to_string)
	}

	/// The extension, lowercase and without the dot.
	#[must_use]
	pub fn extension(&self) -> Option<String> {
		self
			.file_name_path()?
			.extension()?
			.to_str()
			.map(str::to_ascii_lowercase)
	}

	fn file_name_path(&self) -> Option<PathBuf> {
		match self {
			SourceInput::Path(path) => Some(path.clone()),
			SourceInput::Bytes { label, .. } => {
				let without_query = label.split(['?', '#']).next().unwrap_or_default();
				let last = without_query.rsplit('/').next().filter(|s| !s.is_empty())?;
				Some(PathBuf::from(last))
			}
		}
	}
}

impl std::fmt::Debug for SourceInput {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			SourceInput::Path(path) => f.debug_tuple("Path").field(path).finish(),
			SourceInput::Bytes { label, bytes } => f
				.debug_struct("Bytes")
				.field("label", label)
				.field("len", &bytes.len())
				.finish(),
		}
	}
}

impl From<&Path> for SourceInput {
	fn from(path: &Path) -> Self {
		SourceInput::Path(path.to_path_buf())
	}
}

impl From<PathBuf> for SourceInput {
	fn from(path: PathBuf) -> Self {
		SourceInput::Path(path)
	}
}

/// Lets a `Cursor` read a shared `Blob` without copying it.
struct SharedBytes(Arc<Blob>);

impl AsRef<[u8]> for SharedBytes {
	fn as_ref(&self) -> &[u8] {
		self.0.as_slice()
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	fn bytes(label: &str, content: &str) -> SourceInput {
		SourceInput::Bytes {
			label: label.to_string(),
			bytes: Arc::new(Blob::from(content)),
		}
	}

	#[test]
	fn bytes_read_back_whole() {
		let mut text = String::new();
		bytes("x", "a,b\n1,2\n")
			.open()
			.unwrap()
			.read_to_string(&mut text)
			.unwrap();
		assert_eq!(text, "a,b\n1,2\n");
	}

	#[rstest]
	#[case("https://example.org/data/cities.csv", Some("cities"), Some("csv"))]
	#[case("https://example.org/data/Cities.CSV?key=a.b#x", Some("Cities"), Some("csv"))]
	#[case("https://example.org/", None, None)]
	#[case("https://example.org/data/cities", Some("cities"), None)]
	fn a_label_gives_stem_and_extension(
		#[case] label: &str,
		#[case] stem: Option<&str>,
		#[case] extension: Option<&str>,
	) {
		let input = bytes(label, "");
		assert_eq!(input.file_stem().as_deref(), stem);
		assert_eq!(input.extension().as_deref(), extension);
	}

	#[test]
	fn a_path_gives_stem_extension_and_length() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("places.GeoJSON");
		std::fs::write(&path, "{}").unwrap();
		let input = SourceInput::from(path.as_path());
		assert_eq!(input.file_stem().as_deref(), Some("places"));
		assert_eq!(input.extension().as_deref(), Some("geojson"));
		assert_eq!(input.len(), 2);
	}

	#[test]
	fn a_missing_path_names_itself() {
		let error = SourceInput::from(Path::new("missing.csv")).open().err().unwrap();
		assert!(error.to_string().contains("missing.csv"), "got: {error}");
	}
}

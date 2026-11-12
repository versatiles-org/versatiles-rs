//! Where an export goes, and what its file name promises.
//!
//! Shared by every `export` subcommand, so that `-o` means the same in each:
//! a file whose extension says what it holds, or standard output.

use std::{
	io::{IsTerminal, Write},
	path::Path,
};

use anyhow::{Context, Result, bail, ensure};
use versatiles_core::{TileCompression, TileFormat};

/// Refuses to write binary data to a terminal, where it would only garble the
/// screen. Checked before any work, so that a forgotten `-o` costs nothing.
pub(super) fn ensure_stdout_is_not_a_terminal() -> Result<()> {
	ensure!(
		!std::io::stdout().is_terminal(),
		"the output is binary, and standard output is a terminal: write it to a file with -o FILE, \
		 or redirect standard output"
	);
	Ok(())
}

/// Writes `bytes` to `path`, or to standard output without one.
pub(super) fn write(bytes: &[u8], path: Option<&Path>) -> Result<()> {
	if let Some(path) = path {
		return std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()));
	}
	let mut stdout = std::io::stdout().lock();
	stdout.write_all(bytes).context("writing to standard output")?;
	stdout.flush().context("writing to standard output")
}

/// Checks that the extension of `path` names what will be written into it: a
/// file of `format`, compressed with `compression`.
///
/// The extension is the promise a file makes to whoever opens it, and a later
/// `--as` will choose the format by it. So a tile in WebP is not written into
/// `tile.png`, and a gzip-compressed one not into `tile.pbf`. A name without
/// an extension this knows, such as `tile` or `tile.dat`, makes no promise and
/// is accepted.
///
/// `stored` is the compression the tile has in its container, named in the
/// error as the way to get the file the name asks for.
pub(super) fn check_extension(
	path: &Path,
	format: TileFormat,
	compression: TileCompression,
	stored: TileCompression,
) -> Result<()> {
	let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
		return Ok(());
	};
	let mut stem = name.to_string();
	let named_compression = TileCompression::from_filename(&mut stem);
	let named_format = TileFormat::from_filename(&mut stem);

	let matches = match named_format {
		None => named_compression == TileCompression::Uncompressed || named_compression == compression,
		Some(named) => named == format && named_compression == compression,
	};
	if matches {
		return Ok(());
	}

	let expected = format!("{stem}{}{}", format.as_extension(), compression.as_extension());
	let mut message = format!(
		"'{name}' names another kind of file than this export, which is {}: name it '{expected}'",
		describe(format, compression)
	);
	// The name asks for the compression the container stores: that is `--raw`.
	if named_format.is_none_or(|named| named == format) && named_compression == stored && compression != stored {
		message.push_str(", or add --raw to keep the compression the tile is stored with");
	}
	bail!(message)
}

fn describe(format: TileFormat, compression: TileCompression) -> String {
	match compression {
		TileCompression::Uncompressed => format!("{} uncompressed", format.as_str()),
		other => format!("{} compressed with {}", format.as_str(), other.as_str()),
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;
	use versatiles_core::{TileCompression::*, TileFormat::*};

	use super::*;

	#[rstest]
	#[case("tile.pbf", MVT, Uncompressed)]
	#[case("TILE.PBF", MVT, Uncompressed)]
	#[case("dir/14_8801_5373.webp", WEBP, Uncompressed)]
	#[case("tile.jpeg", JPG, Uncompressed)]
	#[case("tile.pbf.gz", MVT, Gzip)]
	#[case("tile.gz", MVT, Gzip)]
	#[case("tile", MVT, Gzip)]
	#[case("tile.dat", WEBP, Uncompressed)]
	fn a_name_that_promises_what_is_written_is_accepted(
		#[case] path: &str,
		#[case] format: TileFormat,
		#[case] compression: TileCompression,
	) {
		check_extension(Path::new(path), format, compression, compression).unwrap();
	}

	#[rstest]
	#[case("tile.png", WEBP, Uncompressed, Uncompressed, "name it 'tile.webp'")]
	#[case("tile.pbf", MVT, Gzip, Gzip, "name it 'tile.pbf.gz'")]
	#[case("tile.pbf.gz", MVT, Uncompressed, Gzip, "or add --raw")]
	#[case("tile.pbf.br", MVT, Uncompressed, Gzip, "name it 'tile.pbf'")]
	#[case("tile.gz", MVT, Uncompressed, Brotli, "name it 'tile.pbf'")]
	fn a_name_that_promises_something_else_is_refused(
		#[case] path: &str,
		#[case] format: TileFormat,
		#[case] compression: TileCompression,
		#[case] stored: TileCompression,
		#[case] needle: &str,
	) {
		let error = check_extension(Path::new(path), format, compression, stored)
			.unwrap_err()
			.to_string();
		assert!(error.contains(needle), "{error}");
	}

	#[test]
	fn raw_is_named_only_where_it_gives_the_file_the_name_asks_for() {
		let error = check_extension(Path::new("tile.pbf.br"), MVT, Uncompressed, Gzip)
			.unwrap_err()
			.to_string();
		assert!(!error.contains("--raw"), "{error}");
	}

	#[test]
	fn write_creates_the_file() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("tile.pbf");
		write(b"bytes", Some(&path))?;
		assert_eq!(std::fs::read(&path)?, b"bytes");
		Ok(())
	}
}

//! Where a conversion's output is built, and how it takes the destination's
//! name.
//!
//! One rule underpins this module: **a destination only ever holds a complete
//! output**. Every writer used to decide for itself what to do with the
//! destination, and the decisions disagreed — one truncated it, one deleted it
//! outright before opening a database, and a later layer moved whatever it found
//! there out of the way. Two of those layers acting on the same run is how a
//! finished conversion came to be replaced by an unfinished one.
//!
//! So writers no longer decide. A writer is handed a path that does not exist,
//! writes a complete container there, and touches nothing else. [`StagedOutput`]
//! owns everything after that: it names the staging location, publishes it with
//! a single `rename`, and removes it if it was never published.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail, ensure};

/// Marker put into the name of an output that finished writing but is missing
/// tiles — before the extension, so the file still opens.
const INCOMPLETE_MARKER: &str = ".incomplete";

/// Suffix of the location an output is built at before it is published.
const STAGING_SUFFIX: &str = ".tmp";

/// A destination being written to.
///
/// Holds a staging location beside the destination and publishes it atomically,
/// or cleans it up. Dropping one that was never published removes whatever the
/// writer left behind.
#[derive(Debug)]
pub(crate) struct StagedOutput {
	/// Where the writer writes. Beside the destination, so the publishing
	/// `rename` stays on one filesystem and is therefore atomic.
	staging: PathBuf,
	/// Where a complete output goes.
	destination: PathBuf,
	/// Set once the staging location has been renamed somewhere; stops `Drop`
	/// from deleting a published output.
	published: bool,
}

impl StagedOutput {
	/// Prepares to write to `destination`.
	///
	/// Clears any staging left behind by an earlier run — a crashed conversion
	/// leaves one, and `SQLite` and the container writers would otherwise add to
	/// it rather than start fresh.
	///
	/// Nothing at `destination` is touched here, or at any point before
	/// [`publish`](Self::publish) succeeds.
	pub fn create(destination: &Path) -> Result<Self> {
		let name = destination
			.file_name()
			.ok_or_else(|| anyhow::anyhow!("cannot write to {destination:?}: it does not name a file or directory"))?;

		let mut staging_name = std::ffi::OsString::from(".");
		staging_name.push(name);
		staging_name.push(STAGING_SUFFIX);
		let staging = destination.with_file_name(staging_name);

		remove_any(&staging).map_err(|e| anyhow::anyhow!("{e} (stale staging from an earlier run)"))?;

		Ok(Self {
			staging,
			destination: destination.to_path_buf(),
			published: false,
		})
	}

	/// Where the writer should write. Does not exist yet.
	pub fn path(&self) -> &Path {
		&self.staging
	}

	/// Replaces the destination with the finished output.
	///
	/// A file is renamed straight over the destination, which is atomic on every
	/// platform this runs on. A directory cannot be — `rename` refuses a
	/// non-empty target — so the old one is moved aside first and removed only
	/// once the new one is in place. A crash in between leaves the previous
	/// output under a `.old` name rather than losing it.
	pub fn publish(mut self) -> Result<()> {
		self.check_writer_produced_something()?;
		self.check_kinds_match()?;

		if self.staging.is_dir() && self.destination.exists() {
			let previous = self.sibling(".old");
			remove_any(&previous)?;
			std::fs::rename(&self.destination, &previous).map_err(|e| {
				anyhow::anyhow!(
					"could not move the previous output {:?} aside to {previous:?}: {e}",
					self.destination
				)
			})?;
			let renamed = std::fs::rename(&self.staging, &self.destination);
			if renamed.is_err() {
				// Put the previous output back rather than leave the destination
				// missing: it is the only copy.
				let _ = std::fs::rename(&previous, &self.destination);
			}
			renamed.map_err(|e| anyhow::anyhow!("could not publish {:?}: {e}", self.destination))?;
			self.published = true;
			remove_any(&previous)?;
			return Ok(());
		}

		std::fs::rename(&self.staging, &self.destination)
			.map_err(|e| anyhow::anyhow!("could not publish {:?}: {e}", self.destination))?;
		self.published = true;
		Ok(())
	}

	/// Publishes under a name that says the output is incomplete, leaving the
	/// destination alone. Returns where it went.
	///
	/// For a run whose writer succeeded but whose *reader* did not: the container
	/// is complete, readable and internally consistent, and simply missing the
	/// tiles that could not be read. That may be hours of work and most of the
	/// data, so deleting it destroys something a person may well want — but
	/// leaving it at the requested path invites every script downstream to treat
	/// it as the conversion it asked for. Renaming settles both.
	pub fn publish_as_incomplete(mut self) -> Result<PathBuf> {
		self.check_writer_produced_something()?;

		let target = incomplete_path(&self.destination)?;
		// `rename` replaces an existing file but not an existing directory, and a
		// leftover from an earlier failed run is exactly what is in the way.
		remove_any(&target)?;
		std::fs::rename(&self.staging, &target)
			.map_err(|e| anyhow::anyhow!("could not keep the incomplete output at {target:?}: {e}"))?;
		self.published = true;
		Ok(target)
	}

	/// A path beside the destination with `suffix` appended to its name.
	fn sibling(&self, suffix: &str) -> PathBuf {
		let mut name = self.destination.file_name().unwrap_or_default().to_os_string();
		name.push(suffix);
		self.destination.with_file_name(name)
	}

	fn check_writer_produced_something(&self) -> Result<()> {
		ensure!(
			self.staging.exists(),
			"the writer reported success but produced nothing at {:?}",
			self.staging
		);
		Ok(())
	}

	/// A file may not replace a directory, or the reverse: that is a mistake
	/// about what the destination is, and carrying it out would delete something
	/// the user did not mean to lose.
	fn check_kinds_match(&self) -> Result<()> {
		if !self.destination.exists() {
			return Ok(());
		}
		let staged_is_dir = self.staging.is_dir();
		let destination_is_dir = self.destination.is_dir();
		if staged_is_dir != destination_is_dir {
			let (new, old) = if staged_is_dir {
				("a directory", "a file")
			} else {
				("a file", "a directory")
			};
			bail!(
				"cannot replace {:?}: it is {old} and this conversion produced {new}",
				self.destination
			);
		}
		Ok(())
	}
}

impl Drop for StagedOutput {
	fn drop(&mut self) {
		if self.published {
			return;
		}
		// Best-effort and logged, never propagated: this runs while an error is
		// on its way up, and must not replace it.
		if let Err(e) = remove_any(&self.staging) {
			log::warn!("could not clean up {:?}: {e}", self.staging);
		}
	}
}

/// The name an output takes when it is complete but missing tiles.
///
/// The marker goes *before* the extension. `out.versatiles.incomplete` names a
/// format nothing recognises, so the container could not be opened without being
/// renamed back — which would make "the data survives" only half true.
/// `out.incomplete.versatiles` still opens, and still cannot be mistaken for the
/// conversion that was asked for.
pub(crate) fn incomplete_path(destination: &Path) -> Result<PathBuf> {
	let name = destination
		.file_name()
		.ok_or_else(|| anyhow::anyhow!("{destination:?} does not name a file or directory"))?;

	let target = if let (Some(stem), Some(extension)) = (destination.file_stem(), destination.extension()) {
		let mut name = stem.to_os_string();
		name.push(INCOMPLETE_MARKER);
		name.push(".");
		name.push(extension);
		name
	} else {
		// A directory output, or a file without an extension.
		let mut name = name.to_os_string();
		name.push(INCOMPLETE_MARKER);
		name
	};
	Ok(destination.with_file_name(target))
}

/// Removes `path`, whether it is a file or a directory, and succeeds if it is
/// already absent.
fn remove_any(path: &Path) -> Result<()> {
	let result = if path.is_dir() {
		std::fs::remove_dir_all(path)
	} else if path.exists() {
		std::fs::remove_file(path)
	} else {
		return Ok(());
	};
	result.map_err(|e| anyhow::anyhow!("could not remove {path:?}: {e}"))
}

#[cfg(test)]
mod tests {
	use assert_fs::TempDir;

	use super::*;

	fn write(path: &Path, contents: &[u8]) {
		if let Some(parent) = path.parent() {
			std::fs::create_dir_all(parent).unwrap();
		}
		std::fs::write(path, contents).unwrap();
	}

	/// C1 — a destination that does not exist.
	#[test]
	fn a_new_destination_is_created_by_publishing() {
		let dir = TempDir::new().unwrap();
		let destination = dir.path().join("out.versatiles");

		let staged = StagedOutput::create(&destination).unwrap();
		assert!(
			!staged.path().exists(),
			"the writer is handed a path that does not exist"
		);
		assert_ne!(staged.path(), destination, "the writer never writes to the destination");
		write(staged.path(), b"new");
		staged.publish().unwrap();

		assert_eq!(std::fs::read(&destination).unwrap(), b"new");
		assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no staging left");
	}

	/// C2 — an existing output is replaced wholesale, and is untouched until the
	/// moment that happens.
	#[test]
	fn an_existing_file_survives_until_the_publish_succeeds() {
		let dir = TempDir::new().unwrap();
		let destination = dir.path().join("out.versatiles");
		write(&destination, b"previous");

		let staged = StagedOutput::create(&destination).unwrap();
		write(staged.path(), b"replacement");
		assert_eq!(
			std::fs::read(&destination).unwrap(),
			b"previous",
			"the previous output must still be there while the new one is being written"
		);

		staged.publish().unwrap();
		assert_eq!(std::fs::read(&destination).unwrap(), b"replacement");
	}

	/// C5 — a failed write. Dropping without publishing is what every error path
	/// does.
	#[test]
	fn an_abandoned_output_cleans_up_and_leaves_the_destination_alone() {
		let dir = TempDir::new().unwrap();
		let destination = dir.path().join("out.versatiles");
		write(&destination, b"previous");

		{
			let staged = StagedOutput::create(&destination).unwrap();
			write(staged.path(), b"half a conversion");
		}

		assert_eq!(std::fs::read(&destination).unwrap(), b"previous");
		assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no staging left");
	}

	/// C7 — a crashed run leaves staging behind. The next run must start clean
	/// rather than add to it.
	#[test]
	fn stale_staging_from_an_earlier_run_is_cleared() {
		let dir = TempDir::new().unwrap();
		let destination = dir.path().join("out.versatiles");
		let stale = dir.path().join(".out.versatiles.tmp");
		write(&stale, b"wreckage");

		let staged = StagedOutput::create(&destination).unwrap();
		assert!(!staged.path().exists(), "stale staging must be gone");
	}

	/// C6 — the writer succeeded but the reader did not.
	#[test]
	fn an_incomplete_output_is_kept_beside_an_untouched_destination() {
		let dir = TempDir::new().unwrap();
		let destination = dir.path().join("out.versatiles");
		write(&destination, b"previous");

		let staged = StagedOutput::create(&destination).unwrap();
		write(staged.path(), b"missing some tiles");
		let kept = staged.publish_as_incomplete().unwrap();

		assert_eq!(kept, dir.path().join("out.incomplete.versatiles"));
		assert_eq!(std::fs::read(&kept).unwrap(), b"missing some tiles");
		assert_eq!(
			std::fs::read(&destination).unwrap(),
			b"previous",
			"the destination must be left exactly as it was"
		);
	}

	/// C4 — the conceptual bug this whole module exists to remove. A directory
	/// output replaces the previous one; it does not merge into it.
	#[test]
	fn a_directory_output_replaces_rather_than_merges() {
		let dir = TempDir::new().unwrap();
		let destination = dir.path().join("tiles");
		write(&destination.join("tiles.json"), b"old metadata");
		write(&destination.join("9/1/1.pbf"), b"a tile from a bigger pyramid");

		let staged = StagedOutput::create(&destination).unwrap();
		write(&staged.path().join("tiles.json"), b"new metadata");
		write(&staged.path().join("3/1/1.pbf"), b"a tile");
		staged.publish().unwrap();

		assert_eq!(std::fs::read(destination.join("tiles.json")).unwrap(), b"new metadata");
		assert!(
			!destination.join("9/1/1.pbf").exists(),
			"a tile from the previous conversion survived into the new output"
		);
		assert!(
			!destination.join("9").exists(),
			"a zoom level from the previous conversion survived"
		);
		assert_eq!(
			std::fs::read_dir(dir.path()).unwrap().count(),
			1,
			"neither staging nor the .old directory may survive"
		);
	}

	/// C3 — the destination is not the kind of thing this conversion produced.
	#[test]
	fn a_file_may_not_replace_a_directory_or_the_reverse() {
		let dir = TempDir::new().unwrap();

		let as_dir = dir.path().join("a");
		write(&as_dir.join("keep.txt"), b"not ours");
		let staged = StagedOutput::create(&as_dir).unwrap();
		write(staged.path(), b"a container file");
		let error = staged.publish().unwrap_err().to_string();
		assert!(error.contains("it is a directory"), "got: {error}");
		assert!(as_dir.join("keep.txt").exists(), "the directory must survive");

		let as_file = dir.path().join("b.versatiles");
		write(&as_file, b"a container file");
		let staged = StagedOutput::create(&as_file).unwrap();
		std::fs::create_dir_all(staged.path()).unwrap();
		let error = staged.publish().unwrap_err().to_string();
		assert!(error.contains("it is a file"), "got: {error}");
		assert_eq!(std::fs::read(&as_file).unwrap(), b"a container file");
	}

	/// A writer that returns `Ok` having written nothing is a bug, and publishing
	/// its absence would delete the destination.
	#[test]
	fn publishing_nothing_is_an_error_and_keeps_the_destination() {
		let dir = TempDir::new().unwrap();
		let destination = dir.path().join("out.versatiles");
		write(&destination, b"previous");

		let staged = StagedOutput::create(&destination).unwrap();
		let error = staged.publish().unwrap_err().to_string();

		assert!(error.contains("produced nothing"), "got: {error}");
		assert_eq!(std::fs::read(&destination).unwrap(), b"previous");
	}

	#[test]
	fn the_incomplete_name_keeps_the_extension() {
		assert_eq!(
			incomplete_path(Path::new("/a/out.versatiles")).unwrap(),
			Path::new("/a/out.incomplete.versatiles"),
			"the container must still open"
		);
		assert_eq!(
			incomplete_path(Path::new("/a/tiles")).unwrap(),
			Path::new("/a/tiles.incomplete")
		);
		assert_eq!(
			incomplete_path(Path::new("/a/out.tar.gz")).unwrap(),
			Path::new("/a/out.tar.incomplete.gz")
		);
	}

	/// Beside the destination, not in `$TMPDIR` — a rename across filesystems is
	/// a copy, and would not be atomic.
	#[test]
	fn the_staging_path_sits_beside_the_destination() {
		let dir = TempDir::new().unwrap();
		let destination = dir.path().join("out.versatiles");
		let staged = StagedOutput::create(&destination).unwrap();
		assert_eq!(staged.path().parent(), destination.parent(), "same filesystem");
		assert_eq!(
			staged.path().file_name().and_then(|n| n.to_str()),
			Some(".out.versatiles.tmp")
		);
	}
}

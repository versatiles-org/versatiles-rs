#![allow(unused)]

use assert_cmd::{Command, cargo};
use std::path::{Path, PathBuf};
use tempfile::{TempDir, tempdir};
use versatiles_core::json::JsonValue;

#[cfg(windows)]
pub const BINARY_NAME: &str = "versatiles.exe";
#[cfg(not(windows))]
pub const BINARY_NAME: &str = "versatiles";

/// Helper to get a testdata file path.
pub fn get_testdata(filename: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.unwrap()
		.join("testdata")
		.join(filename)
}

/// Helper to get a temp output file path.
pub fn get_temp_output(filename: &str) -> (TempDir, PathBuf) {
	let dir = tempdir().expect("failed to create temp dir");
	let path = dir.path().join(filename);
	(dir, path)
}

/// Helper to get tilejson metadata from a file using the CLI.
pub fn get_metadata(filename: &Path) -> String {
	let mut cmd = Command::new(cargo::cargo_bin!());
	let buf = cmd
		.args(["dev", "print-tilejson", filename.to_str().unwrap()])
		.output()
		.unwrap()
		.stdout;
	String::from_utf8(buf).unwrap().replace('"', "")
}

pub fn path_to_string(path: &Path) -> String {
	JsonValue::from(path.to_string_lossy().to_string()).stringify()
}

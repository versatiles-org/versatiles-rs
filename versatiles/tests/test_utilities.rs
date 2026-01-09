#![allow(unused)]

use std::{
	path::{Path, PathBuf},
	process::{Command, Stdio},
};
use tempfile::{TempDir, tempdir};
use versatiles_core::json::JsonValue;

#[cfg(windows)]
pub const BINARY_NAME: &str = "versatiles.exe";
#[cfg(not(windows))]
pub const BINARY_NAME: &str = "versatiles";

/// Helper to get a testdata file path.
pub fn get_testdata(filename: &str) -> String {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.unwrap()
		.join("testdata")
		.join(filename)
		.to_str()
		.unwrap()
		.to_string()
}

/// Helper to get a temp output file path.
pub fn get_temp_output(filename: &str) -> (TempDir, PathBuf) {
	let dir = tempdir().expect("failed to create temp dir");
	let path = dir.path().join(filename);
	(dir, path)
}

/// Helper to create a Command for the versatiles binary.
pub fn versatiles_cmd() -> Command {
	let path = assert_cmd::cargo::cargo_bin!();
	let mut cmd = Command::new(path);
	cmd.stdout(Stdio::piped());
	cmd.stderr(Stdio::piped());
	cmd.stdin(Stdio::piped());
	cmd
}

pub struct VersaTilesResult {
	pub success: bool,
	pub code: i32,
	pub stdout: String,
	pub stderr: String,
}

pub fn versatiles_output(args: &str) -> VersaTilesResult {
	let mut cmd = versatiles_cmd();
	if !args.is_empty() {
		cmd.args(args.split(' '));
	}
	let output = cmd.output().unwrap();
	VersaTilesResult {
		success: output.status.success(),
		code: output.status.code().unwrap(),
		stdout: String::from_utf8(output.stdout).unwrap(),
		stderr: String::from_utf8(output.stderr).unwrap(),
	}
}

pub fn versatiles_run(args: &str) {
	let o = versatiles_output(args);
	assert!(o.success, "command failed: {}\nstderr: {}", args, o.stderr);
	assert_eq!(o.code, 0, "unexpected exit code: {}", o.code);
	assert!(o.stdout.is_empty(), "expected empty stdout, got: {}", o.stdout);
}

pub fn versatiles_stdin(args: &str, stdin: &str) {
	let mut cmd = versatiles_cmd();
	if !args.is_empty() {
		cmd.args(args.split(' '));
	}

	let mut child = cmd.spawn().expect("failed to spawn versatiles command");
	use std::io::Write;
	child
		.stdin
		.as_mut()
		.expect("failed to open stdin")
		.write_all(stdin.as_bytes())
		.expect("failed to write to stdin");
	let output = child.wait_with_output().expect("failed to read stdout");
	assert!(
		output.status.success(),
		"command failed: {}\nstderr: {}",
		args,
		String::from_utf8_lossy(&output.stderr)
	);
	assert_eq!(output.status.code().unwrap(), 0, "unexpected exit code");
	assert!(output.stdout.is_empty(), "expected empty stdout");
}

/// Helper to get tilejson metadata from a file using the CLI.
pub fn get_tilejson(filename: &Path) -> JsonValue {
	let mut cmd = versatiles_cmd();
	let output = cmd
		.args(["dev", "print-tilejson", filename.to_str().unwrap()])
		.output()
		.unwrap()
		.stdout;

	JsonValue::parse_str(&String::from_utf8(output).unwrap()).unwrap()
}

#[macro_export]
macro_rules! assert_contains {
	($left:expr, $right:expr$(,)?) => ({
		$crate::assert_contains!(@ $left, $right, "", "");
	});
	($left:expr, $right:expr, $($arg:tt)*) => ({
		$crate::assert_contains!(@ $left, $right, ": ", $($arg)+);
	});
	(@ $left:expr, $right:expr, $maybe_colon:expr, $($arg:tt)*) => ({
		let left_val = String::from($left);
		let right_val = String::from($right);
		if !(left_val.contains(&right_val)) {
			::core::panic!("assertion failed: `(left == right)`{}{}\\n\\n{}\\n",
				$maybe_colon,
				format_args!($($arg)*),
				pretty_assertions::Comparison::new(&left_val, &right_val)
			)
		}
	});
}

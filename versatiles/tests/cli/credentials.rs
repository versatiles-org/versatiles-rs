//! A password written into a URL on the command line must not come back out
//! of the process: not in a log line at any verbosity, not in an error.
//!
//! Every remote here is a closed local port, so each run fails fast and the
//! test only looks at what was printed on the way.

use rstest::rstest;

use crate::test_utilities::*;

const SECRET: &str = "s3cretpw";

/// Run `versatiles -vvv <args>` with retries off and return everything it printed.
fn printed(args: &[&str]) -> String {
	let output = versatiles_cmd()
		.arg("-vvv")
		.args(args)
		.env("VERSATILES_NET_MAX_RETRIES", "0")
		.output()
		.unwrap();
	assert!(!output.status.success(), "a closed port should fail: {args:?}");
	format!(
		"{}{}",
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr)
	)
}

#[rstest]
#[case::https("https://alice:s3cretpw@127.0.0.1:9/x.versatiles")]
#[case::sftp("sftp://alice:s3cretpw@127.0.0.1:9/x.versatiles")]
#[case::bracket_prefix("[osm]sftp://alice:s3cretpw@127.0.0.1:9/x.versatiles")]
fn a_password_in_the_input_is_never_printed(#[case] input: &str) {
	for args in [vec!["probe", input], vec!["convert", input, "out.versatiles"]] {
		let out = printed(&args);
		assert!(!out.contains(SECRET), "password printed by {args:?}:\n{out}");
		// The account is still named, so the failure stays explainable.
		assert_contains!(out, "alice@127.0.0.1:9");
	}
}

#[test]
fn a_password_in_the_output_is_never_printed() {
	let input = get_testdata("berlin.mbtiles");
	let args = ["convert", &input, "sftp://bob:s3cretpw@127.0.0.1:9/out.versatiles"];
	let out = printed(&args);
	assert!(!out.contains(SECRET), "password printed by {args:?}:\n{out}");
	assert_contains!(out, "bob@127.0.0.1:9");
}

mod test_utilities;
use predicates::str;
use rstest::rstest;
use test_utilities::*;

#[test]
fn command() {
	let o = versatiles_output("");
	assert!(!o.success);
	assert_eq!(o.code, 2);
	assert!(o.stdout.is_empty());
	assert!(o.stderr.contains(&format!("Usage: {BINARY_NAME} [OPTIONS] <COMMAND>")));
}

#[rstest]
#[case("convert", "[OPTIONS] <INPUT_FILE> <OUTPUT_FILE>")]
#[case("dev export-outline", "[OPTIONS] <INPUT_FILE> <OUTPUT_FILE>")]
#[case("dev measure-tile-sizes", "[OPTIONS] <INPUT_FILE> <OUTPUT_FILE> [LEVEL] [SCALE]")]
#[case("dev print-tilejson", "[OPTIONS] <INPUT_FILE>")]
#[case("dev", "[OPTIONS] <COMMAND>")]
#[case("help", "[OPTIONS] <COMMAND>")]
#[case("probe", "[OPTIONS] <FILENAME>")]
#[case("serve", "[OPTIONS] [TILE_SOURCES]...")]
fn subcommand(#[case] sub_command: &str, #[case] usage: &str) {
	let o = versatiles_output(sub_command);
	assert!(!o.success);
	assert_eq!(o.code, 2);
	assert!(o.stdout.is_empty());
	assert_contains!(o.stderr, &format!("Usage: {BINARY_NAME} {sub_command} {usage}"));
}

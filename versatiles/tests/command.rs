mod test_utilities;
use predicates::str;
use rstest::rstest;
use test_utilities::*;

#[test]
fn command() -> Result<(), Box<dyn std::error::Error>> {
	versatiles_cmd()
		.assert()
		.failure()
		.code(2)
		.stdout(str::is_empty())
		.stderr(str::contains(format!("Usage: {BINARY_NAME} [OPTIONS] <COMMAND>")));
	Ok(())
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
fn subcommand(#[case] sub_command: &str, #[case] usage: &str) -> Result<(), Box<dyn std::error::Error>> {
	versatiles_cmd()
		.args(sub_command.split(" "))
		.assert()
		.failure()
		.code(2)
		.stdout(str::is_empty())
		.stderr(str::contains(format!("Usage: {BINARY_NAME} {sub_command} {usage}")));
	Ok(())
}

mod test_utilities;
use predicates::str;
use rstest::rstest;
use test_utilities::*;

#[rstest]
#[case("help pipeline", "VersaTiles Pipeline")]
#[case("help --raw pipeline", "^# VersaTiles Pipeline")]
#[case("help config", "VersaTiles Server Configuration")]
#[case("help --raw config", "^# VersaTiles Server Configuration")]
fn help_command(#[case] sub_command: &str, #[case] pattern: &str) -> Result<(), Box<dyn std::error::Error>> {
	versatiles_cmd()
		.args(sub_command.split(" "))
		.assert()
		.success()
		.stdout(str::is_match(pattern).unwrap())
		.stderr(str::is_empty());
	Ok(())
}

use assert_cmd::{Command, cargo};
use predicates::str;
use rstest::rstest;

#[rstest]
#[case("help pipeline", "VersaTiles Pipeline")]
#[case("help --raw pipeline", "^# VersaTiles Pipeline")]
#[case("help config", "VersaTiles Server Configuration")]
#[case("help --raw config", "^# VersaTiles Server Configuration")]
fn help_command(#[case] sub_command: &str, #[case] pattern: &str) -> Result<(), Box<dyn std::error::Error>> {
	Command::new(cargo::cargo_bin!())
		.args(sub_command.split(" "))
		.assert()
		.success()
		.stdout(str::is_match(pattern).unwrap())
		.stderr(str::is_empty());
	Ok(())
}

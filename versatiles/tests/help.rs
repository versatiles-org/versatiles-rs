#![cfg(feature = "cli")]

mod test_utilities;
use predicates::str;
use rstest::rstest;
use test_utilities::*;

#[rstest]
#[case("help pipeline", "VersaTiles Pipeline")]
#[case("help --raw pipeline", "# VersaTiles Pipeline\n")]
#[case("help config", "VersaTiles Server Configuration")]
#[case("help --raw config", "# VersaTiles Server Configuration\n")]
fn e2e_help_command(#[case] sub_command: &str, #[case] pattern: &str) -> Result<(), Box<dyn std::error::Error>> {
	let o = versatiles_output(sub_command);
	assert!(o.success, "command failed: {}\nstderr: {}", sub_command, o.stderr);
	assert_eq!(o.code, 0, "unexpected exit code: {}", o.code);
	assert!(o.stderr.is_empty(), "expected empty stderr, got: {}", o.stderr);
	assert_contains!(o.stdout, pattern);
	Ok(())
}

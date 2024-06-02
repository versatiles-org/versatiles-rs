use anyhow::Result;

use crate::container;

#[derive(clap::Args, Debug)]
#[command(
	arg_required_else_help = true,
	disable_help_flag = true,
	disable_version_flag = true
)]
pub struct Subcommand {
	#[command(subcommand)]
	topic: Topic,
}

#[derive(clap::Subcommand, Debug)]
enum Topic {
	Composer,
}

pub fn run(command: &Subcommand) -> Result<()> {
	match command.topic {
		Topic::Composer => eprintln!("{}", container::get_composer_operation_docs()),
	};
	Ok(())
}

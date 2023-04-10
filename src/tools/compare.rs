use crate::container::get_reader;
use clap::Args;

#[derive(Args)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	file1: String,
	file2: String,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand) -> Result<()> {
	println!("compare {:?} with {:?}", arguments.file1, arguments.file2);

	let _reader1 = get_reader(&arguments.file1);
	let _reader2 = get_reader(&arguments.file2);
	todo!()
}

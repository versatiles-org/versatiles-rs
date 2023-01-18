use crate::opencloudtiles::tools::get_reader;
use clap::Args;

#[derive(Args)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// tile container you want to probe
	/// supported container formats are: *.cloudtiles, *.tar, *.mbtiles
	#[arg(required = true, verbatim_doc_comment)]
	file: String,
}

pub fn run(arguments: &Subcommand) {
	println!("probe {:?}", arguments.file);

	let reader = get_reader(&arguments.file);
	println!("{:#?}", reader);
}

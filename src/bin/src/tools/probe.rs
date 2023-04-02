use clap::Args;
use futures::executor::block_on;
use versatiles_container::get_reader;

#[derive(Args)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// tile container you want to probe
	/// supported container formats are: *.versatiles, *.tar, *.mbtiles
	#[arg(required = true, verbatim_doc_comment)]
	file: String,

	/// deep scan of every tile
	#[arg(long, short)]
	deep: bool,
}

pub fn run(arguments: &Subcommand) {
	block_on(async {
		println!("probe {:?}", arguments.file);

		let reader = get_reader(&arguments.file).await.unwrap();
		println!("{reader:#?}");

		if arguments.deep {
			reader.deep_verify().await;
		}
	})
}

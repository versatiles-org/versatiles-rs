use crate::{containers::get_reader, shared::Result};
use clap::Args;

#[derive(Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// tile container you want to probe
	/// supported container formats are: *.versatiles, *.tar, *.mbtiles
	#[arg(required = true, verbatim_doc_comment)]
	filename: String,
	/*
		/// deep scan of every tile
		#[arg(long, short)]
		deep: bool,
	*/
}

#[tokio::main]
pub async fn run(arguments: &Subcommand) -> Result<()> {
	eprintln!("probe {:?}", arguments.filename);

	let reader = get_reader(&arguments.filename).await?;
	eprintln!("{reader:#?}");

	Ok(())
}

#[cfg(test)]
mod tests {
	use crate::tests::run_command;

	#[test]
	#[cfg(feature = "mbtiles")]
	fn test_local() {
		run_command(vec!["versatiles", "probe", "testdata/berlin.mbtiles"]).unwrap();
	}

	#[test]
	fn test_remote() {
		run_command(vec![
			"versatiles",
			"probe",
			"https://download.versatiles.org/planet-latest.versatiles",
		])
		.unwrap();
	}
}

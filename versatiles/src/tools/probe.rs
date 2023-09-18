use clap::{ArgAction::Count, Args};
use versatiles_lib::{containers::get_reader, shared::Result};

#[derive(Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// tile container you want to probe
	/// supported container formats are: *.versatiles, *.tar, *.mbtiles
	#[arg(required = true, verbatim_doc_comment)]
	filename: String,

	/// deep scan (if supported yet)
	/// -d scans container
	/// -dd scans every tile
	#[arg(long, short, action = Count,)]
	deep: u8,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand) -> Result<()> {
	eprintln!("probe {:?}", arguments.filename);

	let mut reader = get_reader(&arguments.filename).await?;
	reader.probe(arguments.deep).await?;

	Ok(())
}

#[allow(unused_imports)]
#[cfg(test)]
mod tests {
	use crate::tests::run_command;

	#[test]

	fn test_local() {
		run_command(vec!["versatiles", "probe", "-q", "../testdata/berlin.mbtiles"]).unwrap();
	}

	#[test]

	fn test_remote() {
		run_command(vec![
			"versatiles",
			"probe",
			"-q",
			"https://download.versatiles.org/planet-latest.versatiles",
		])
		.unwrap();
	}
}

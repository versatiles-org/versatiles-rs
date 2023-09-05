use crate::{containers::get_reader, shared::Result};
use clap::{ArgAction::Count, Args};

#[derive(Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// tile container you want to probe
	/// supported container formats are: *.versatiles, *.tar, *.mbtiles
	#[arg(required = true, verbatim_doc_comment)]
	filename: String,

	/// scan deep
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
	#[cfg(feature = "mbtiles")]
	fn test_local() {
		run_command(vec!["versatiles", "probe", "testdata/berlin.mbtiles"]).unwrap();
	}

	#[test]
	#[cfg(feature = "request")]
	fn test_remote() {
		run_command(vec![
			"versatiles",
			"probe",
			"https://download.versatiles.org/planet-latest.versatiles",
		])
		.unwrap();
	}
}
